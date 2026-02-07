//! INBV - In-Boot Video Driver Interface
//!
//! Подсистема ядра для управления boot video.
//! Предоставляет абстракцию над bootvid для использования в KD и HAL.
//!
//! # Архитектура
//!
//! ```text
//! KD / HAL / Kernel
//!       │
//!       v
//!     INBV (этот модуль)
//!       │
//!       │ imports::bootvid (IAT)
//!       v
//!   BOOTVID.DLL (динамически загружается)
//! ```

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use crate::imports::bootvid;
use crate::imports::bootvid::FramebufferInfo;
use crate::imports::bootvid::BOOTCHAR_HEIGHT;
use crate::imports::bootvid::BOOTCHAR_WIDTH;
use crate::imports::bootvid::BV_COLOR_BLACK;
use crate::imports::bootvid::BV_COLOR_WHITE;

use crate::hal::irql::DISPATCH_LEVEL;
use crate::hal::irql::KIRQL;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock_raise_to;
use crate::ke::spinlock::ke_try_to_acquire_spin_lock_raise_to;

// =============================================================================
// Display State
// =============================================================================

/// Состояние дисплея
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InbvDisplayState {
    /// NT: мы владеем дисплеем
    Owned = 0,
    /// NT: мы владеем, но использовать нельзя
    Disabled = 1,
    /// NT: владение потеряно
    Lost = 2,
}

impl From<u32> for InbvDisplayState {
    fn from(val: u32) -> Self {
        match val {
            0 => InbvDisplayState::Owned,
            1 => InbvDisplayState::Disabled,
            2 => InbvDisplayState::Lost,
            _ => InbvDisplayState::Disabled, // fail-safe
        }
    }
}

/// Callback: сброс параметров дисплея при возвращении владения (NT: Cols/Rows)
pub type InbvResetDisplayParameters = fn(cols: u32, rows: u32) -> bool;

/// Фильтр строки перед выводом (NT: получает указатель на строку и может модифицировать)
pub type InbvDisplayStringFilter = fn(s: &mut &str);

// =============================================================================
// Глобальное состояние INBV
// =============================================================================

/// Текущее состояние дисплея
static INBV_DISPLAY_STATE: AtomicU32 = AtomicU32::new(InbvDisplayState::Disabled as u32);

/// Boot driver установлен
static INBV_BOOT_DRIVER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Вывод debug строк разрешен
static INBV_DISPLAY_DEBUG_STRINGS: AtomicBool = AtomicBool::new(false);

/// Callback для InbvNotifyDisplayOwnershipLost
static mut INBV_RESET_DISPLAY_PARAMETERS: Option<InbvResetDisplayParameters> = None;

/// Display string filter (если установлен)
static mut INBV_DISPLAY_FILTER: Option<InbvDisplayStringFilter> = None;

/// Lock для синхронизации доступа к дисплею.
static INBV_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Состояние для пары `inbv_acquire_lock` / `inbv_release_lock`.
static mut INBV_LOCK_OLD_IRQL: KIRQL = 0;
static mut INBV_LOCK_RAISED: bool = false;

/// Размеры framebuffer (сохраняются при инициализации)
static mut FB_WIDTH: u32 = 0;
static mut FB_HEIGHT: u32 = 0;

/// Guard для INBV lock, восстанавливающий IRQL при Drop.
struct InbvLockGuard {
    old_irql: KIRQL,
    raised: bool,
}

impl Drop for InbvLockGuard {
    fn drop(&mut self) {
        INBV_LOCK.release();
        if self.raised {
            crate::hal::irql::kf_lower_irql(self.old_irql);
        }
    }
}

// =============================================================================
// Инициализация
// =============================================================================

/// Инициализация INBV драйвера
///
/// # Arguments
/// * `fb_info` - информация о framebuffer
///
/// # Returns
/// `true` если инициализация успешна
pub fn inbv_driver_initialize(fb_info: &FramebufferInfo) -> bool {
    // NT/ReactOS: если уже установлен — считаем успехом
    if inbv_is_boot_driver_installed() {
        return true;
    }

    // Сохраняем размеры framebuffer
    unsafe {
        FB_WIDTH = fb_info.width as u32;
        FB_HEIGHT = fb_info.height as u32;
    }

    // Инициализируем bootvid через IAT
    let result = unsafe { bootvid::initialize(fb_info) };

    if result {
        INBV_BOOT_DRIVER_INSTALLED.store(true, Ordering::Release);
        // При установке драйвера считаем дисплей доступным (OWNED).
        INBV_DISPLAY_STATE.store(InbvDisplayState::Owned as u32, Ordering::Release);
    }

    result
}

/// Проверка установки boot driver
#[inline]
pub fn inbv_is_boot_driver_installed() -> bool {
    INBV_BOOT_DRIVER_INSTALLED.load(Ordering::Acquire)
}

/// Получить ширину framebuffer
#[inline]
pub fn inbv_get_fb_width() -> u32 {
    unsafe { FB_WIDTH }
}

/// Получить высоту framebuffer
#[inline]
pub fn inbv_get_fb_height() -> u32 {
    unsafe { FB_HEIGHT }
}

// =============================================================================
// Display Ownership
// =============================================================================

/// Захват владения дисплеем
pub fn inbv_acquire_display_ownership() {
    let prev = inbv_get_display_state();

    // NT/ReactOS: если был LOST и есть callback — вызываем его
    if prev == InbvDisplayState::Lost {
        let cb = unsafe { INBV_RESET_DISPLAY_PARAMETERS };
        if let Some(cb) = cb {
            let fb_w = inbv_get_fb_width();
            let fb_h = inbv_get_fb_height();
            let cols = if fb_w != 0 {
                fb_w / (BOOTCHAR_WIDTH as u32)
            } else {
                80
            };
            let rows = if fb_h != 0 {
                fb_h / (BOOTCHAR_HEIGHT as u32)
            } else {
                50
            };
            let _ = cb(cols.max(1), rows.max(1));
        }
    }

    INBV_DISPLAY_STATE.store(InbvDisplayState::Owned as u32, Ordering::Release);
}

/// Освобождение владения дисплеем
pub fn inbv_release_display_ownership() {
    INBV_DISPLAY_STATE.store(InbvDisplayState::Lost as u32, Ordering::Release);
}

/// NT/ReactOS: проверка владения дисплеем (TRUE если дисплей не LOST)
#[inline]
pub fn inbv_check_display_ownership() -> bool {
    let state = InbvDisplayState::from(INBV_DISPLAY_STATE.load(Ordering::Acquire));
    state != InbvDisplayState::Lost
}

/// Получить текущее состояние дисплея
#[inline]
pub fn inbv_get_display_state() -> InbvDisplayState {
    InbvDisplayState::from(INBV_DISPLAY_STATE.load(Ordering::Acquire))
}

#[inline]
fn inbv_is_display_owned() -> bool {
    inbv_get_display_state() == InbvDisplayState::Owned
}

/// NT: InbvEnableBootDriver — переключает Owned/Disabled (если не LOST)
pub fn inbv_enable_boot_driver(enable: bool) {
    if inbv_get_display_state() == InbvDisplayState::Lost {
        return;
    }

    if inbv_is_boot_driver_installed() {
        inbv_acquire_lock();
        if inbv_get_display_state() == InbvDisplayState::Owned {
            unsafe { bootvid::cleanup() };
        }
        INBV_DISPLAY_STATE.store(
            if enable {
                InbvDisplayState::Owned as u32
            } else {
                InbvDisplayState::Disabled as u32
            },
            Ordering::Release,
        );
        inbv_release_lock();
    } else {
        INBV_DISPLAY_STATE.store(
            if enable {
                InbvDisplayState::Owned as u32
            } else {
                InbvDisplayState::Disabled as u32
            },
            Ordering::Release,
        );
    }
}

/// NT: InbvSetDisplayOwnership — переключает Owned/Lost
pub fn inbv_set_display_ownership(display_owned: bool) {
    INBV_DISPLAY_STATE.store(
        if display_owned {
            InbvDisplayState::Owned as u32
        } else {
            InbvDisplayState::Lost as u32
        },
        Ordering::Release,
    );
}

/// NT: InbvNotifyDisplayOwnershipLost — сохраняет callback и переводит в LOST (+ cleanup)
pub fn inbv_notify_display_ownership_lost(callback: InbvResetDisplayParameters) {
    if inbv_is_boot_driver_installed() {
        inbv_acquire_lock();
        if inbv_get_display_state() != InbvDisplayState::Lost {
            unsafe { bootvid::cleanup() };
        }
        unsafe {
            INBV_RESET_DISPLAY_PARAMETERS = Some(callback);
        }
        INBV_DISPLAY_STATE.store(InbvDisplayState::Lost as u32, Ordering::Release);
        inbv_release_lock();
    } else {
        unsafe {
            INBV_RESET_DISPLAY_PARAMETERS = Some(callback);
        }
        INBV_DISPLAY_STATE.store(InbvDisplayState::Lost as u32, Ordering::Release);
    }
}

/// NT: InbvInstallDisplayStringFilter — установить фильтр строк
pub fn inbv_install_display_string_filter(filter: InbvDisplayStringFilter) {
    unsafe {
        INBV_DISPLAY_FILTER = Some(filter);
    }
}

// =============================================================================
// Display String Control
// =============================================================================

/// Включить/выключить вывод debug строк
pub fn inbv_enable_display_string(enable: bool) -> bool {
    let prev = INBV_DISPLAY_DEBUG_STRINGS.swap(enable, Ordering::AcqRel);
    prev
}

/// Проверка разрешения вывода debug строк
#[inline]
pub fn inbv_display_string_enabled() -> bool {
    INBV_DISPLAY_DEBUG_STRINGS.load(Ordering::Acquire)
}

// =============================================================================
// Locking
// =============================================================================

/// NT: InbvTestLock — попытаться захватить lock без блокировки
pub fn inbv_test_lock() -> bool {
    let current = crate::hal::irql::ke_get_current_irql();

    if current <= DISPATCH_LEVEL {
        let Some(old_irql) = ke_try_to_acquire_spin_lock_raise_to(&INBV_LOCK, DISPATCH_LEVEL)
        else {
            return false;
        };
        unsafe {
            INBV_LOCK_OLD_IRQL = old_irql;
            INBV_LOCK_RAISED = true;
        }
        true
    } else {
        if INBV_LOCK.try_acquire() {
            unsafe {
                INBV_LOCK_OLD_IRQL = current;
                INBV_LOCK_RAISED = false;
            }
            true
        } else {
            false
        }
    }
}

/// Захват lock для синхронизации
pub fn inbv_acquire_lock() {
    let current = crate::hal::irql::ke_get_current_irql();
    if current <= DISPATCH_LEVEL {
        let old_irql = ke_acquire_spin_lock_raise_to(&INBV_LOCK, DISPATCH_LEVEL);
        unsafe {
            INBV_LOCK_OLD_IRQL = old_irql;
            INBV_LOCK_RAISED = true;
        }
    } else {
        INBV_LOCK.acquire();
        unsafe {
            INBV_LOCK_OLD_IRQL = current;
            INBV_LOCK_RAISED = false;
        }
    }
}

/// Освобождение lock
pub fn inbv_release_lock() {
    INBV_LOCK.release();
    unsafe {
        if INBV_LOCK_RAISED {
            crate::hal::irql::kf_lower_irql(INBV_LOCK_OLD_IRQL);
        }
    }
}

/// Пытается захватить INBV lock и вернуть guard.
fn inbv_try_acquire_lock() -> Option<InbvLockGuard> {
    let current = crate::hal::irql::ke_get_current_irql();
    if current <= DISPATCH_LEVEL {
        let old_irql = ke_try_to_acquire_spin_lock_raise_to(&INBV_LOCK, DISPATCH_LEVEL)?;
        Some(InbvLockGuard {
            old_irql,
            raised: true,
        })
    } else {
        if INBV_LOCK.try_acquire() {
            Some(InbvLockGuard {
                old_irql: current,
                raised: false,
            })
        } else {
            None
        }
    }
}

/// Захватывает INBV lock блокирующим образом и возвращает guard.
fn inbv_acquire_lock_guard() -> InbvLockGuard {
    loop {
        if let Some(g) = inbv_try_acquire_lock() {
            return g;
        }
        while INBV_LOCK.is_locked() {
            crate::arch::x86_64::cpu::yield_processor();
        }
    }
}

// =============================================================================
// Display Functions
// =============================================================================

/// Вывод строки на экран
pub fn inbv_display_string(s: &str) -> bool {
    if !inbv_is_boot_driver_installed() {
        return false;
    }

    if !inbv_is_display_owned() {
        return false;
    }

    if !inbv_display_string_enabled() {
        return true;
    }

    let mut s = s;
    unsafe {
        if let Some(filter) = INBV_DISPLAY_FILTER {
            filter(&mut s);
        }
    }

    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::display_string(s) };

    true
}

/// Best-effort версия `InbvDisplayString`.
pub fn inbv_display_string_best_effort(s: &str) -> bool {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return false;
    }
    if !inbv_display_string_enabled() {
        return true;
    }

    let mut s = s;
    unsafe {
        if let Some(filter) = INBV_DISPLAY_FILTER {
            filter(&mut s);
        }
    }

    let Some(_guard) = inbv_try_acquire_lock() else {
        return false;
    };
    unsafe { bootvid::display_string(s) };
    true
}

/// Вывод строки в указанной позиции
pub fn inbv_display_string_xy(s: &str, left: u32, top: u32) -> bool {
    inbv_display_string_xy_ex(s, left, top, true)
}

/// Расширенная версия `InbvDisplayStringXY` с флагом прозрачности
pub fn inbv_display_string_xy_ex(s: &str, left: u32, top: u32, transparent: bool) -> bool {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return false;
    }

    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::display_string_xy(s, left, top, transparent) };

    true
}

/// Установка цвета текста
pub fn inbv_set_text_color(color: u8) -> u8 {
    if !inbv_is_boot_driver_installed() {
        return 0;
    }

    unsafe { bootvid::set_text_color(color) }
}

/// Заливка прямоугольной области цветом
pub fn inbv_solid_color_fill(left: u32, top: u32, right: u32, bottom: u32, color: u8) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }

    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::solid_color_fill(left, top, right, bottom, color) };
}

/// Установка области прокрутки
pub fn inbv_set_scroll_region(left: u32, top: u32, right: u32, bottom: u32) {
    if !inbv_is_boot_driver_installed() {
        return;
    }

    unsafe { bootvid::set_scroll_region(left, top, right, bottom) };
}

/// Сброс дисплея
pub fn inbv_reset_display() -> bool {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return false;
    }

    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::reset_display(false) };

    true
}

// =============================================================================
// BLT functions
// =============================================================================

pub fn inbv_bit_blt(buffer: *const u8, x: u32, y: u32) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }
    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::bit_blt(buffer, x, y) };
}

pub fn inbv_buffer_to_screen_blt(
    buffer: *const u8,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }
    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::buffer_to_screen_blt(buffer, x, y, width, height, delta) };
}

pub fn inbv_screen_to_buffer_blt(
    buffer: *mut u8,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }
    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::screen_to_buffer_blt(buffer, x, y, width, height, delta) };
}

// =============================================================================
// Вспомогательные функции для инициализации
// =============================================================================

/// Подготовка экрана для вывода debug информации
pub fn inbv_setup_display_for_debug(width: u32, height: u32) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }

    inbv_solid_color_fill(0, 0, width, height, BV_COLOR_BLACK);
    inbv_set_text_color(BV_COLOR_WHITE);
    inbv_set_scroll_region(0, 0, width, height);
    inbv_enable_display_string(true);
}

// =============================================================================
// XenOS Extensions (для кастомных тем)
// =============================================================================

/// Заливка прямоугольника прямым BGRA32 цветом (мимо палитры)
pub fn inbv_solid_color_fill_bgra(left: u32, top: u32, right: u32, bottom: u32, bgra: u32) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }
    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::solid_color_fill_bgra(left, top, right, bottom, bgra) };
}

/// Установка цвета фона текста прямым BGRA32
pub fn inbv_set_text_background_bgra(bgra: u32) {
    if !inbv_is_boot_driver_installed() || !inbv_is_display_owned() {
        return;
    }
    let _guard = inbv_acquire_lock_guard();
    unsafe { bootvid::set_text_background_bgra(bgra) };
}
