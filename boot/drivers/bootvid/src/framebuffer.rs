//! Framebuffer Backend
//!
//! Linear Framebuffer для UEFI/Limine (32bpp BGRA).

use core::ptr::null_mut;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

// =============================================================================
// FramebufferInfo - информация о framebuffer от загрузчика
// =============================================================================

/// Информация о framebuffer (передается из Limine)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// Адрес framebuffer в памяти
    pub address: *mut u32,

    /// Ширина в пикселях
    pub width: usize,

    /// Высота в пикселях
    pub height: usize,

    /// Pitch (байт на строку)
    pub pitch: usize,

    /// Bits per pixel (обычно 32)
    pub bpp: u8,
}

impl FramebufferInfo {
    /// Создать пустую структуру
    pub const fn empty() -> Self {
        Self {
            address: null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
        }
    }

    /// Проверка валидности
    pub fn is_valid(&self) -> bool {
        !self.address.is_null() && self.width > 0 && self.height > 0 && self.bpp >= 24
    }

    /// Количество пикселей на строку (pitch / 4 для 32bpp)
    pub fn pixels_per_row(&self) -> usize {
        self.pitch / 4
    }
}

unsafe impl Send for FramebufferInfo {}
unsafe impl Sync for FramebufferInfo {}

// =============================================================================
// Глобальное состояние framebuffer
// =============================================================================

/// Framebuffer инициализирован
static FB_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Адрес framebuffer
static FB_ADDRESS: AtomicPtr<u32> = AtomicPtr::new(null_mut());

/// Ширина экрана
static FB_WIDTH: AtomicUsize = AtomicUsize::new(0);

/// Высота экрана  
static FB_HEIGHT: AtomicUsize = AtomicUsize::new(0);

/// Pitch (байт на строку)
static FB_PITCH: AtomicUsize = AtomicUsize::new(0);

/// Пикселей на строку
static FB_PIXELS_PER_ROW: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Публичный API framebuffer
// =============================================================================

/// Инициализация framebuffer
pub fn fb_initialize(info: &FramebufferInfo) -> bool {
    if !info.is_valid() {
        return false;
    }

    FB_ADDRESS.store(info.address, Ordering::Release);
    FB_WIDTH.store(info.width, Ordering::Release);
    FB_HEIGHT.store(info.height, Ordering::Release);
    FB_PITCH.store(info.pitch, Ordering::Release);
    FB_PIXELS_PER_ROW.store(info.pixels_per_row(), Ordering::Release);
    FB_INITIALIZED.store(true, Ordering::Release);

    true
}

/// Проверка инициализации
#[inline]
pub fn fb_is_initialized() -> bool {
    FB_INITIALIZED.load(Ordering::Acquire)
}

/// Получить ширину экрана
#[inline]
pub fn fb_get_width() -> usize {
    FB_WIDTH.load(Ordering::Acquire)
}

/// Получить высоту экрана
#[inline]
pub fn fb_get_height() -> usize {
    FB_HEIGHT.load(Ordering::Acquire)
}

/// Получить pitch
#[inline]
pub fn fb_get_pitch() -> usize {
    FB_PITCH.load(Ordering::Acquire)
}

/// Получить пикселей на строку
#[inline]
pub fn fb_get_pixels_per_row() -> usize {
    FB_PIXELS_PER_ROW.load(Ordering::Acquire)
}

/// Установить пиксель по координатам
///
/// # Safety
/// Координаты должны быть в пределах экрана
#[inline]
pub fn fb_set_pixel(x: usize, y: usize, color: u32) {
    if !fb_is_initialized() {
        return;
    }

    let width = fb_get_width();
    let height = fb_get_height();

    if x >= width || y >= height {
        return;
    }

    let pixels_per_row = fb_get_pixels_per_row();
    let addr = FB_ADDRESS.load(Ordering::Acquire);

    if addr.is_null() {
        return;
    }

    unsafe {
        let offset = y * pixels_per_row + x;
        addr.add(offset).write_volatile(color);
    }
}

/// Получить пиксель по координатам
#[inline]
pub fn fb_get_pixel(x: usize, y: usize) -> u32 {
    if !fb_is_initialized() {
        return 0;
    }

    let width = fb_get_width();
    let height = fb_get_height();

    if x >= width || y >= height {
        return 0;
    }

    let pixels_per_row = fb_get_pixels_per_row();
    let addr = FB_ADDRESS.load(Ordering::Acquire);

    if addr.is_null() {
        return 0;
    }

    unsafe {
        let offset = y * pixels_per_row + x;
        addr.add(offset).read_volatile()
    }
}

/// Заливка прямоугольника цветом
pub fn fb_fill_rect(left: usize, top: usize, right: usize, bottom: usize, color: u32) {
    if !fb_is_initialized() {
        return;
    }

    let width = fb_get_width();
    let height = fb_get_height();
    let pixels_per_row = fb_get_pixels_per_row();
    let addr = FB_ADDRESS.load(Ordering::Acquire);

    if addr.is_null() {
        return;
    }

    // Clamp coordinates
    let left = left.min(width);
    let right = right.min(width);
    let top = top.min(height);
    let bottom = bottom.min(height);

    if left >= right || top >= bottom {
        return;
    }

    unsafe {
        for y in top..bottom {
            let row_start = addr.add(y * pixels_per_row + left);
            for x in 0..(right - left) {
                row_start.add(x).write_volatile(color);
            }
        }
    }
}

/// Копирование области экрана (для скроллинга)
///
/// Оптимизировано для быстрого скроллинга - копирует строки целиком
pub fn fb_copy_rect(
    src_x: usize,
    src_y: usize,
    dst_x: usize,
    dst_y: usize,
    width: usize,
    height: usize,
) {
    if !fb_is_initialized() || width == 0 || height == 0 {
        return;
    }

    let screen_width = fb_get_width();
    let screen_height = fb_get_height();
    let pixels_per_row = fb_get_pixels_per_row();
    let addr = FB_ADDRESS.load(Ordering::Acquire);

    if addr.is_null() {
        return;
    }

    // Проверка границ
    if src_x + width > screen_width || src_y + height > screen_height {
        return;
    }
    if dst_x + width > screen_width || dst_y + height > screen_height {
        return;
    }

    unsafe {
        // Определяем направление копирования для предотвращения перезаписи
        if dst_y < src_y || (dst_y == src_y && dst_x <= src_x) {
            // Копируем сверху вниз - быстрое копирование строк
            for y in 0..height {
                let src_row = addr.add((src_y + y) * pixels_per_row + src_x);
                let dst_row = addr.add((dst_y + y) * pixels_per_row + dst_x);
                // Копируем строку целиком (width * 4 байта)
                core::ptr::copy(src_row, dst_row, width);
            }
        } else {
            // Копируем снизу вверх
            for y in (0..height).rev() {
                let src_row = addr.add((src_y + y) * pixels_per_row + src_x);
                let dst_row = addr.add((dst_y + y) * pixels_per_row + dst_x);
                core::ptr::copy(src_row, dst_row, width);
            }
        }
    }
}

/// Получить прямой указатель на framebuffer (для оптимизированных операций)
#[inline]
pub fn fb_get_address() -> *mut u32 {
    FB_ADDRESS.load(Ordering::Acquire)
}
