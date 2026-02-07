//! Work Items - отложенное выполнение в контексте потока
//!
//! Источники:
//! - NT5: ex/work.c
//! - ReactOS: ex/work.c

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock;
use crate::ke::spinlock::ke_release_spin_lock;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;

/// Тип очереди работ
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WORK_QUEUE_TYPE {
    /// Критические работы - высокий приоритет
    CriticalWorkQueue = 0,
    /// Отложенные работы - нормальный приоритет
    DelayedWorkQueue = 1,
    /// Гипер-критические - очень высокий приоритет
    HyperCriticalWorkQueue = 2,
    /// Максимум
    MaximumWorkQueue = 3,
}

/// Тип функции work item
pub type PWORKER_THREAD_ROUTINE = extern "win64" fn(parameter: PVOID);

/// WORK_QUEUE_ITEM - элемент очереди работ
#[repr(C)]
pub struct WORK_QUEUE_ITEM {
    /// Элемент списка
    pub list: LIST_ENTRY,
    /// Функция для выполнения
    pub worker_routine: Option<PWORKER_THREAD_ROUTINE>,
    /// Параметр для функции
    pub parameter: PVOID,
}

impl WORK_QUEUE_ITEM {
    pub const fn new() -> Self {
        Self {
            list: LIST_ENTRY::new(),
            worker_routine: None,
            parameter: core::ptr::null_mut(),
        }
    }
}

impl Default for WORK_QUEUE_ITEM {
    fn default() -> Self {
        Self::new()
    }
}

/// EX_WORK_QUEUE - очередь работ
pub struct EX_WORK_QUEUE {
    /// Спинлок для синхронизации
    pub lock: KSPIN_LOCK,
    /// Список работ
    pub work_items_list_head: LIST_ENTRY,
    /// Количество работ в очереди
    pub work_items_count: AtomicU32,
    /// Количество рабочих потоков
    pub worker_threads: AtomicU32,
    /// Семафор для сигнализации (количество работ)
    pub semaphore_count: AtomicU32,
}

impl EX_WORK_QUEUE {
    pub const fn new() -> Self {
        Self {
            lock: KSPIN_LOCK::new(),
            work_items_list_head: LIST_ENTRY::new(),
            work_items_count: AtomicU32::new(0),
            worker_threads: AtomicU32::new(0),
            semaphore_count: AtomicU32::new(0),
        }
    }

    /// Инициализирует очередь
    ///
    /// # Safety
    /// Должен вызываться один раз
    pub unsafe fn init(&mut self) {
        unsafe {
            LIST_ENTRY::init_head(&mut self.work_items_list_head as *mut LIST_ENTRY);
        }
    }
}

impl Default for EX_WORK_QUEUE {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Глобальные очереди
// =============================================================================

use crate::ke::init::GlobalData;

/// Очередь критических работ
static CRITICAL_WORK_QUEUE: GlobalData<EX_WORK_QUEUE> = GlobalData::new(EX_WORK_QUEUE::new());

/// Очередь отложенных работ
static DELAYED_WORK_QUEUE: GlobalData<EX_WORK_QUEUE> = GlobalData::new(EX_WORK_QUEUE::new());

/// Очередь гипер-критических работ
static HYPER_CRITICAL_WORK_QUEUE: GlobalData<EX_WORK_QUEUE> = GlobalData::new(EX_WORK_QUEUE::new());

/// Получает очередь по типу
fn get_work_queue(queue_type: WORK_QUEUE_TYPE) -> *mut EX_WORK_QUEUE {
    unsafe {
        match queue_type {
            WORK_QUEUE_TYPE::CriticalWorkQueue => CRITICAL_WORK_QUEUE.get(),
            WORK_QUEUE_TYPE::DelayedWorkQueue => DELAYED_WORK_QUEUE.get(),
            WORK_QUEUE_TYPE::HyperCriticalWorkQueue => HYPER_CRITICAL_WORK_QUEUE.get(),
            WORK_QUEUE_TYPE::MaximumWorkQueue => core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// API
// =============================================================================

/// ExInitializeWorkItem - инициализирует work item
///
/// # Arguments
/// * `item` - work item для инициализации
/// * `routine` - функция для выполнения
/// * `context` - параметр для функции
pub fn ex_initialize_work_item(
    item: &mut WORK_QUEUE_ITEM,
    routine: PWORKER_THREAD_ROUTINE,
    context: PVOID,
) {
    item.list = LIST_ENTRY::new();
    item.worker_routine = Some(routine);
    item.parameter = context;
}

/// ExQueueWorkItem - ставит work item в очередь
///
/// # Arguments
/// * `work_item` - work item для выполнения
/// * `queue_type` - тип очереди
pub fn ex_queue_work_item(work_item: &mut WORK_QUEUE_ITEM, queue_type: WORK_QUEUE_TYPE) {
    let queue = get_work_queue(queue_type);
    if queue.is_null() {
        return;
    }

    unsafe {
        let old_irql = ke_acquire_spin_lock(&(*queue).lock);

        // Добавляем в конец списка
        LIST_ENTRY::insert_tail(
            &mut (*queue).work_items_list_head as *mut LIST_ENTRY,
            &mut work_item.list as *mut LIST_ENTRY,
        );

        (*queue).work_items_count.fetch_add(1, Ordering::SeqCst);
        (*queue).semaphore_count.fetch_add(1, Ordering::SeqCst);

        ke_release_spin_lock(&(*queue).lock, old_irql);
    }

    // TODO: Сигнализировать рабочим потокам
}

/// Извлекает work item из очереди (для worker thread)
///
/// # Safety
/// Должен вызываться только из worker thread
pub unsafe fn exp_dequeue_work_item(queue_type: WORK_QUEUE_TYPE) -> Option<*mut WORK_QUEUE_ITEM> {
    let queue = get_work_queue(queue_type);
    if queue.is_null() {
        return None;
    }

    unsafe {
        let old_irql = ke_acquire_spin_lock(&(*queue).lock);

        if LIST_ENTRY::is_empty(&(*queue).work_items_list_head as *const LIST_ENTRY) {
            ke_release_spin_lock(&(*queue).lock, old_irql);
            return None;
        }

        // Извлекаем из начала списка
        let entry = LIST_ENTRY::remove_head(&mut (*queue).work_items_list_head as *mut LIST_ENTRY);

        (*queue).work_items_count.fetch_sub(1, Ordering::SeqCst);

        ke_release_spin_lock(&(*queue).lock, old_irql);

        // Преобразуем LIST_ENTRY в WORK_QUEUE_ITEM
        Some(crate::containing_record!(entry, WORK_QUEUE_ITEM, list))
    }
}

/// Обрабатывает work item
///
/// # Safety
/// work_item должен быть валиден
pub unsafe fn exp_process_work_item(work_item: *mut WORK_QUEUE_ITEM) {
    if work_item.is_null() {
        return;
    }

    unsafe {
        if let Some(routine) = (*work_item).worker_routine {
            let param = (*work_item).parameter;
            routine(param);
        }
    }
}

/// Возвращает количество работ в очереди
pub fn ex_query_work_queue_depth(queue_type: WORK_QUEUE_TYPE) -> u32 {
    let queue = get_work_queue(queue_type);
    if queue.is_null() {
        return 0;
    }

    unsafe { (*queue).work_items_count.load(Ordering::SeqCst) }
}

// =============================================================================
// IO_WORKITEM - расширенный work item для драйверов
// =============================================================================

/// IO_WORKITEM - work item с привязкой к device object
#[repr(C)]
pub struct IO_WORKITEM {
    /// Базовый work item
    pub work_item: WORK_QUEUE_ITEM,
    /// Device object
    pub device_object: PVOID,
    /// IO routine
    pub io_routine: Option<PIO_WORKITEM_ROUTINE>,
    /// Контекст
    pub context: PVOID,
    /// Тип очереди
    pub queue_type: WORK_QUEUE_TYPE,
}

/// Тип функции IO work item
pub type PIO_WORKITEM_ROUTINE = extern "win64" fn(device_object: PVOID, context: PVOID);

impl IO_WORKITEM {
    pub const fn new() -> Self {
        Self {
            work_item: WORK_QUEUE_ITEM::new(),
            device_object: core::ptr::null_mut(),
            io_routine: None,
            context: core::ptr::null_mut(),
            queue_type: WORK_QUEUE_TYPE::DelayedWorkQueue,
        }
    }
}

impl Default for IO_WORKITEM {
    fn default() -> Self {
        Self::new()
    }
}

/// IoAllocateWorkItem - выделяет IO work item
pub fn io_allocate_work_item(device_object: PVOID) -> Option<*mut IO_WORKITEM> {
    let ptr = super::pool::ex_allocate_pool_with_tag(
        super::pool::POOL_TYPE::NonPagedPool,
        core::mem::size_of::<IO_WORKITEM>(),
        u32::from_le_bytes(*b"IoWk"),
    );

    if ptr.is_null() {
        return None;
    }

    let item = ptr as *mut IO_WORKITEM;
    unsafe {
        (*item).work_item = WORK_QUEUE_ITEM::new();
        (*item).device_object = device_object;
        (*item).io_routine = None;
        (*item).context = core::ptr::null_mut();
        (*item).queue_type = WORK_QUEUE_TYPE::DelayedWorkQueue;
    }

    Some(item)
}

/// IoFreeWorkItem - освобождает IO work item
pub fn io_free_work_item(io_work_item: *mut IO_WORKITEM) {
    if !io_work_item.is_null() {
        super::pool::ex_free_pool_with_tag(io_work_item as PVOID, u32::from_le_bytes(*b"IoWk"));
    }
}

/// IoQueueWorkItem - ставит IO work item в очередь
pub fn io_queue_work_item(
    io_work_item: *mut IO_WORKITEM,
    worker_routine: PIO_WORKITEM_ROUTINE,
    queue_type: WORK_QUEUE_TYPE,
    context: PVOID,
) {
    if io_work_item.is_null() {
        return;
    }

    unsafe {
        (*io_work_item).io_routine = Some(worker_routine);
        (*io_work_item).context = context;
        (*io_work_item).queue_type = queue_type;

        // Wrapper routine
        extern "win64" fn io_work_item_wrapper(parameter: PVOID) {
            let item = parameter as *mut IO_WORKITEM;
            if item.is_null() {
                return;
            }
            unsafe {
                if let Some(routine) = (*item).io_routine {
                    routine((*item).device_object, (*item).context);
                }
            }
        }

        ex_initialize_work_item(
            &mut (*io_work_item).work_item,
            io_work_item_wrapper,
            io_work_item as PVOID,
        );

        ex_queue_work_item(&mut (*io_work_item).work_item, queue_type);
    }
}

// =============================================================================
// Инициализация
// =============================================================================

/// Инициализирует work queue subsystem
///
/// # Safety
/// Должен вызываться один раз при инициализации
pub unsafe fn exp_init_work_queues() {
    unsafe {
        (*CRITICAL_WORK_QUEUE.get()).init();
        (*DELAYED_WORK_QUEUE.get()).init();
        (*HYPER_CRITICAL_WORK_QUEUE.get()).init();
    }
}
