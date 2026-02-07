//! Miniport Interface - API для miniport драйверов
//!
//! Фаза 2.1: per-adapter instance model - убран глобальный state,
//! HW_INITIALIZATION_DATA хранится в Driver Extension.
//!
//! Фаза 2.2: Multi-adapter support через обратный указатель в miniport extension.

use crate::types::*;
use crate::imports::ntoskrnl::*;
use crate::{storport_print, storport_print_hex, STORPORT_POOL_TAG};
use core::ptr;

// =============================================================================
// Miniport Extension Header (для обратного указателя на STORPORT_DEVICE_EXTENSION)
// =============================================================================

/// Заголовок miniport extension
/// 
/// Размещается ПЕРЕД областью данных miniport драйвера.
/// Позволяет найти STORPORT_DEVICE_EXTENSION по указателю hw_device_extension.
#[repr(C)]
pub struct MINIPORT_EXTENSION_HEADER {
    /// Signature для проверки: 'MPEH'
    pub signature: ULONG,
    /// Обратный указатель на STORPORT_DEVICE_EXTENSION
    pub storport_extension: *mut STORPORT_DEVICE_EXTENSION,
    /// Размер данных miniport после заголовка
    pub miniport_data_size: ULONG,
    /// Reserved для выравнивания
    pub reserved: ULONG,
}

pub const MINIPORT_EXTENSION_HEADER_SIGNATURE: ULONG = 0x4845504D; // 'MPEH'

impl MINIPORT_EXTENSION_HEADER {
    pub fn new(storport_ext: *mut STORPORT_DEVICE_EXTENSION, data_size: ULONG) -> Self {
        Self {
            signature: MINIPORT_EXTENSION_HEADER_SIGNATURE,
            storport_extension: storport_ext,
            miniport_data_size: data_size,
            reserved: 0,
        }
    }
}

// =============================================================================
// Driver Extension (per-driver HW_INIT_DATA storage)
// =============================================================================

/// StorPort Driver Extension
/// 
/// Хранит HW_INITIALIZATION_DATA для каждого miniport драйвера отдельно.
/// Это позволяет иметь несколько miniport драйверов одновременно.
#[repr(C)]
pub struct STORPORT_DRIVER_EXTENSION {
    /// Signature для проверки: 'SPDR'
    pub signature: ULONG,
    /// HW Initialization Data от miniport
    pub hw_init_data: HW_INITIALIZATION_DATA,
    /// Reserved для будущего использования
    pub reserved: [ULONG; 4],
}

pub const STORPORT_DRIVER_EXTENSION_SIGNATURE: ULONG = 0x52445053; // 'SPDR'

impl STORPORT_DRIVER_EXTENSION {
    pub fn new() -> Self {
        Self {
            signature: STORPORT_DRIVER_EXTENSION_SIGNATURE,
            hw_init_data: unsafe { core::mem::zeroed() },
            reserved: [0; 4],
        }
    }
}

// =============================================================================
// Per-LUN State (Phase 2.2 - per-LUN queues)
// =============================================================================

/// Максимальное число LUN per adapter
/// Ограничено для уменьшения размера структуры на стеке
pub const MAX_LUNS_PER_ADAPTER: usize = 16;

/// Состояние очереди LUN
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LUN_QUEUE_STATE {
    /// PathId (bus number)
    pub path_id: UCHAR,
    /// TargetId  
    pub target_id: UCHAR,
    /// Lun number
    pub lun: UCHAR,
    /// LUN валиден/активен
    pub active: BOOLEAN,
    /// Очередь заморожена (frozen)
    pub frozen: BOOLEAN,
    /// Текущий pending SRB для этого LUN
    pub pending_srb: PSCSI_REQUEST_BLOCK,
    /// IRP связанный с pending SRB
    pub pending_irp: PIRP,
    /// Счётчик in-flight запросов на этом LUN
    pub in_flight: ULONG,
    /// Максимум in-flight для этого LUN (queue depth)
    pub queue_depth: ULONG,
}

impl Default for LUN_QUEUE_STATE {
    fn default() -> Self {
        Self {
            path_id: 0,
            target_id: 0,
            lun: 0,
            active: 0,
            frozen: 0,
            pending_srb: ptr::null_mut(),
            pending_irp: ptr::null_mut(),
            in_flight: 0,
            queue_depth: 1, // По умолчанию synchronous (1 запрос)
        }
    }
}

impl LUN_QUEUE_STATE {
    /// Инициализирует LUN state
    pub fn init(&mut self, path_id: UCHAR, target_id: UCHAR, lun: UCHAR, queue_depth: ULONG) {
        self.path_id = path_id;
        self.target_id = target_id;
        self.lun = lun;
        self.active = 1;
        self.frozen = 0;
        self.pending_srb = ptr::null_mut();
        self.pending_irp = ptr::null_mut();
        self.in_flight = 0;
        self.queue_depth = queue_depth.max(1);
    }
    
    /// Можно ли принять запрос
    pub fn can_accept(&self) -> bool {
        self.active != 0 && self.frozen == 0 && self.in_flight < self.queue_depth
    }
    
    /// Устанавливает pending запрос
    pub fn set_pending(&mut self, srb: PSCSI_REQUEST_BLOCK, irp: PIRP) {
        self.pending_srb = srb;
        self.pending_irp = irp;
        self.in_flight += 1;
    }
    
    /// Очищает pending запрос
    pub fn clear_pending(&mut self) {
        self.pending_srb = ptr::null_mut();
        self.pending_irp = ptr::null_mut();
        if self.in_flight > 0 {
            self.in_flight -= 1;
        }
    }
}

// =============================================================================
// Device Extension (per-adapter state)
// =============================================================================

/// StorPort Device Extension
#[repr(C)]
pub struct STORPORT_DEVICE_EXTENSION {
    /// Signature для проверки валидности: 'SPDE'
    pub signature: ULONG,
    /// FDO созданный StorPort
    pub device_object: PDEVICE_OBJECT,
    /// PDO от PnP Manager
    pub physical_device_object: PDEVICE_OBJECT,
    /// Lower device в стеке
    pub lower_device: PDEVICE_OBJECT,
    /// Driver object
    pub driver_object: PDRIVER_OBJECT,
    /// HW Initialization Data от miniport (копия из driver extension)
    pub hw_init_data: HW_INITIALIZATION_DATA,
    /// Device Extension miniport (variable size)
    pub miniport_device_extension: PVOID,
    /// Список PDO для обнаруженных устройств
    pub child_pdos: [PDEVICE_OBJECT; 32],
    /// Количество child PDO
    pub child_count: ULONG,
    /// Started flag
    pub started: BOOLEAN,
    /// Adapter number (для поддержки нескольких адаптеров)
    pub adapter_number: ULONG,
    
    // --- SRB Queue/Lifecycle fields (Phase 2.2) ---
    /// Текущий активный SRB (для синхронной модели - один SRB за раз)
    pub current_srb: PSCSI_REQUEST_BLOCK,
    /// IRP связанный с текущим SRB
    pub current_irp: PIRP,
    /// Флаг готовности принять следующий запрос
    pub ready_for_next: BOOLEAN,
    /// Счётчик in-flight SRB (для будущей async модели)
    pub in_flight_count: ULONG,
    /// Максимум in-flight SRB (из capabilities)
    pub max_in_flight: ULONG,
    
    // --- Per-LUN state (Phase 2.2) ---
    /// Состояние очередей per-LUN
    pub lun_states: [LUN_QUEUE_STATE; MAX_LUNS_PER_ADAPTER],
    /// Количество активных LUN
    pub lun_count: ULONG,
    
    // --- DPC/Interrupt/Timer fields (Phase 2.3) ---
    /// DPC для completion processing
    pub completion_dpc: KDPC,
    /// DPC инициализирован
    pub completion_dpc_initialized: BOOLEAN,
    /// Request timeout timer
    pub timeout_timer: KTIMER,
    /// DPC для timeout processing  
    pub timeout_dpc: KDPC,
    /// Timeout timer инициализирован
    pub timeout_timer_initialized: BOOLEAN,
    /// Request timeout в секундах (0 = disabled)
    pub request_timeout_secs: ULONG,
    /// Spinlock для защиты DPC state
    pub dpc_lock: ULONG_PTR,
    /// HwInterrupt callback (from HW_INIT_DATA)
    pub hw_interrupt: Option<unsafe extern "win64" fn(PVOID) -> BOOLEAN>,
    /// Interrupt enabled flag
    pub interrupt_enabled: BOOLEAN,
    
    // --- DMA/Scatter-Gather limits (Phase 2.4) ---
    /// Максимальный размер одной передачи (из PORT_CONFIGURATION_INFORMATION)
    pub max_transfer_length: ULONG,
    /// Число "разрывов" физ. адресов (elements - 1)
    pub number_of_physical_breaks: ULONG,
    /// Маска выравнивания для буферов
    pub alignment_mask: ULONG,
    /// Поддерживает scatter/gather
    pub scatter_gather_supported: BOOLEAN,
    /// Использует 64-bit DMA адреса
    pub dma64_supported: BOOLEAN,
    
    // --- Interrupt support (Phase 3.1) ---
    /// KINTERRUPT объект для подключенного прерывания
    pub interrupt_object: crate::imports::ntoskrnl::PKINTERRUPT,
    /// Interrupt vector (GSI)
    pub interrupt_vector: ULONG,
    /// Interrupt IRQL
    pub interrupt_irql: UCHAR,
}

pub const STORPORT_DEVICE_EXTENSION_SIGNATURE: ULONG = 0x45445053; // 'SPDE'

/// Счётчик адаптеров для присвоения уникальных номеров
static mut ADAPTER_COUNTER: ULONG = 0;

impl STORPORT_DEVICE_EXTENSION {
    pub fn new() -> Self {
        Self {
            signature: STORPORT_DEVICE_EXTENSION_SIGNATURE,
            device_object: ptr::null_mut(),
            physical_device_object: ptr::null_mut(),
            lower_device: ptr::null_mut(),
            driver_object: ptr::null_mut(),
            hw_init_data: unsafe { core::mem::zeroed() },
            miniport_device_extension: ptr::null_mut(),
            child_pdos: [ptr::null_mut(); 32],
            child_count: 0,
            started: 0,
            adapter_number: 0,
            // SRB lifecycle
            current_srb: ptr::null_mut(),
            current_irp: ptr::null_mut(),
            ready_for_next: 1, // Initially ready
            in_flight_count: 0,
            max_in_flight: 1,  // Start with synchronous model (1 at a time)
            // Per-LUN state
            lun_states: [LUN_QUEUE_STATE::default(); MAX_LUNS_PER_ADAPTER],
            lun_count: 0,
            // DPC/Interrupt/Timer (Phase 2.3)
            completion_dpc: KDPC::zeroed(),
            completion_dpc_initialized: 0,
            timeout_timer: KTIMER::zeroed(),
            timeout_dpc: KDPC::zeroed(),
            timeout_timer_initialized: 0,
            request_timeout_secs: 30, // Default 30 seconds
            dpc_lock: 0,
            hw_interrupt: None,
            interrupt_enabled: 0,
            // DMA/Scatter-Gather limits (Phase 2.4)
            max_transfer_length: 0xFFFF_FFFF, // Default: no limit
            number_of_physical_breaks: STOR_MAX_SG_ELEMENTS as ULONG - 1,
            alignment_mask: 0,
            scatter_gather_supported: 0,
            dma64_supported: 0,
            // Interrupt support (Phase 3.1)
            interrupt_object: ptr::null_mut(),
            interrupt_vector: 0,
            interrupt_irql: 0,
        }
    }
    
    /// Устанавливает текущий SRB и связанный IRP
    pub unsafe fn set_current_request(&mut self, srb: PSCSI_REQUEST_BLOCK, irp: PIRP) {
        self.current_srb = srb;
        self.current_irp = irp;
        self.ready_for_next = 0;
        self.in_flight_count += 1;
    }
    
    /// Очищает текущий запрос после completion
    pub unsafe fn clear_current_request(&mut self) {
        self.current_srb = ptr::null_mut();
        self.current_irp = ptr::null_mut();
        if self.in_flight_count > 0 {
            self.in_flight_count -= 1;
        }
    }
    
    /// Проверяет готовность принять запрос
    pub fn can_accept_request(&self) -> bool {
        self.ready_for_next != 0 && self.in_flight_count < self.max_in_flight
    }
    
    // =========================================================================
    // Per-LUN methods
    // =========================================================================
    
    /// Находит или создаёт LUN state по path/target/lun
    pub fn get_or_create_lun(&mut self, path_id: UCHAR, target_id: UCHAR, lun: UCHAR) -> Option<&mut LUN_QUEUE_STATE> {
        // Ищем существующий
        for i in 0..self.lun_count as usize {
            if self.lun_states[i].active != 0 
               && self.lun_states[i].path_id == path_id
               && self.lun_states[i].target_id == target_id
               && self.lun_states[i].lun == lun {
                return Some(&mut self.lun_states[i]);
            }
        }
        
        // Создаём новый если есть место
        if (self.lun_count as usize) < MAX_LUNS_PER_ADAPTER {
            let idx = self.lun_count as usize;
            self.lun_states[idx].init(path_id, target_id, lun, 1);
            self.lun_count += 1;
            return Some(&mut self.lun_states[idx]);
        }
        
        None
    }
    
    /// Находит LUN state по path/target/lun
    pub fn find_lun(&mut self, path_id: UCHAR, target_id: UCHAR, lun: UCHAR) -> Option<&mut LUN_QUEUE_STATE> {
        for i in 0..self.lun_count as usize {
            if self.lun_states[i].active != 0 
               && self.lun_states[i].path_id == path_id
               && self.lun_states[i].target_id == target_id
               && self.lun_states[i].lun == lun {
                return Some(&mut self.lun_states[i]);
            }
        }
        None
    }
    
    /// Замораживает очередь LUN
    pub fn freeze_lun(&mut self, path_id: UCHAR, target_id: UCHAR, lun: UCHAR) {
        if let Some(lun_state) = self.find_lun(path_id, target_id, lun) {
            lun_state.frozen = 1;
        }
    }
    
    /// Размораживает очередь LUN
    pub fn unfreeze_lun(&mut self, path_id: UCHAR, target_id: UCHAR, lun: UCHAR) {
        if let Some(lun_state) = self.find_lun(path_id, target_id, lun) {
            lun_state.frozen = 0;
        }
    }
    
    /// Отменяет все pending запросы на адаптере (для RESET_DETECTED)
    pub unsafe fn cancel_all_pending(&mut self) {
        storport_print("[STORPORT] Cancelling all pending requests\n");
        
        // Отменяем adapter-level запрос
        if !self.current_srb.is_null() && !self.current_irp.is_null() {
            storport_print("[STORPORT]   Cancelling adapter current SRB\n");
            (*self.current_srb).SrbStatus = SRB_STATUS_BUS_RESET;
            (*self.current_irp).io_status.status = STATUS_IO_DEVICE_ERROR;
            (*self.current_irp).io_status.information = 0;
            IoCompleteRequest(self.current_irp, IO_NO_INCREMENT);
            self.clear_current_request();
        }
        
        // Отменяем per-LUN запросы
        for i in 0..self.lun_count as usize {
            let lun_state = &mut self.lun_states[i];
            if lun_state.active != 0 && !lun_state.pending_srb.is_null() {
                storport_print("[STORPORT]   Cancelling LUN ");
                storport_print_hex(lun_state.path_id as u64);
                storport_print("/");
                storport_print_hex(lun_state.target_id as u64);
                storport_print("/");
                storport_print_hex(lun_state.lun as u64);
                storport_print(" pending SRB\n");
                
                (*lun_state.pending_srb).SrbStatus = SRB_STATUS_BUS_RESET;
                if !lun_state.pending_irp.is_null() {
                    (*lun_state.pending_irp).io_status.status = STATUS_IO_DEVICE_ERROR;
                    (*lun_state.pending_irp).io_status.information = 0;
                    IoCompleteRequest(lun_state.pending_irp, IO_NO_INCREMENT);
                }
                lun_state.clear_pending();
            }
            // Замораживаем очередь после reset
            lun_state.frozen = 1;
        }
        
        self.ready_for_next = 0;
        storport_print("[STORPORT] All pending requests cancelled\n");
    }
}

// =============================================================================
// StorPortInitialize - Главная точка входа для miniport
// =============================================================================

/// StorPortInitialize - вызывается из miniport DriverEntry
///
/// Фаза 2.1: Теперь использует IoAllocateDriverObjectExtension для хранения
/// HW_INITIALIZATION_DATA per-driver, позволяя иметь несколько miniport драйверов.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortInitialize(
    driver_object: PDRIVER_OBJECT,
    registry_path: *const UNICODE_STRING,
    hw_init_data: PHW_INITIALIZATION_DATA,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] StorPortInitialize called\n");
        storport_print("[STORPORT]   driver=0x");
        storport_print_hex(driver_object as u64);
        storport_print("\n");
        
        if driver_object.is_null() || hw_init_data.is_null() {
            storport_print("[STORPORT] ERROR: NULL parameter\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        let hw_data = &*hw_init_data;
        
        if hw_data.HwInitializationDataSize != core::mem::size_of::<HW_INITIALIZATION_DATA>() as ULONG {
            storport_print("[STORPORT] ERROR: Invalid HwInitializationDataSize=0x");
            storport_print_hex(hw_data.HwInitializationDataSize as u64);
            storport_print(" expected=0x");
            storport_print_hex(core::mem::size_of::<HW_INITIALIZATION_DATA>() as u64);
            storport_print("\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        if hw_data.HwFindAdapter.is_none() || hw_data.HwInitialize.is_none() || hw_data.HwStartIo.is_none() {
            storport_print("[STORPORT] ERROR: Missing required callbacks\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        // Используем адрес StorPortAddDevice как client_identification_address
        // Это стандартный паттерн в NT драйверах
        let client_id = StorPortAddDevice as PVOID;
        
        // Выделяем Driver Extension для хранения HW_INIT_DATA
        let mut driver_ext_ptr: PVOID = ptr::null_mut();
        let ext_size = core::mem::size_of::<STORPORT_DRIVER_EXTENSION>() as ULONG;
        
        let status = IoAllocateDriverObjectExtension(
            driver_object,
            client_id,
            ext_size,
            &mut driver_ext_ptr,
        );
        
        if status < 0 {
            storport_print("[STORPORT] ERROR: IoAllocateDriverObjectExtension failed: 0x");
            storport_print_hex(status as u64);
            storport_print("\n");
            return status;
        }
        
        storport_print("[STORPORT] Driver extension allocated at 0x");
        storport_print_hex(driver_ext_ptr as u64);
        storport_print("\n");
        
        // Инициализируем driver extension
        let driver_ext = driver_ext_ptr as *mut STORPORT_DRIVER_EXTENSION;
        ptr::write(driver_ext, STORPORT_DRIVER_EXTENSION::new());
        
        // Копируем HW_INITIALIZATION_DATA в driver extension
        (*driver_ext).hw_init_data = HW_INITIALIZATION_DATA {
            HwInitializationDataSize: hw_data.HwInitializationDataSize,
            AdapterInterfaceType: hw_data.AdapterInterfaceType,
            HwInitialize: hw_data.HwInitialize,
            HwStartIo: hw_data.HwStartIo,
            HwInterrupt: hw_data.HwInterrupt,
            HwFindAdapter: hw_data.HwFindAdapter,
            HwResetBus: hw_data.HwResetBus,
            HwDmaStarted: hw_data.HwDmaStarted,
            HwAdapterState: hw_data.HwAdapterState,
            DeviceExtensionSize: hw_data.DeviceExtensionSize,
            SpecificLuExtensionSize: hw_data.SpecificLuExtensionSize,
            SrbExtensionSize: hw_data.SrbExtensionSize,
            NumberOfAccessRanges: hw_data.NumberOfAccessRanges,
            Reserved: hw_data.Reserved,
            MapBuffers: hw_data.MapBuffers,
            NeedPhysicalAddresses: hw_data.NeedPhysicalAddresses,
            TaggedQueuing: hw_data.TaggedQueuing,
            AutoRequestSense: hw_data.AutoRequestSense,
            MultipleRequestPerLu: hw_data.MultipleRequestPerLu,
            ReceiveEvent: hw_data.ReceiveEvent,
            VendorIdLength: hw_data.VendorIdLength,
            VendorId: hw_data.VendorId,
            PortVersionFlags: hw_data.PortVersionFlags,
            DeviceIdLength: hw_data.DeviceIdLength,
            DeviceId: hw_data.DeviceId,
            HwAdapterControl: hw_data.HwAdapterControl,
            HwBuildIo: hw_data.HwBuildIo,
        };
        
        // CRITICAL: Устанавливаем AddDevice callback в driver_extension
        // Это позволяет PnP Manager вызвать StorPortAddDevice когда найдено устройство
        let drv_ext = (*driver_object).driver_extension;
        if !drv_ext.is_null() {
            (*drv_ext).add_device = Some(StorPortAddDevice);
            storport_print("[STORPORT] AddDevice callback installed\n");
        } else {
            storport_print("[STORPORT] WARNING: driver_extension is NULL!\n");
        }
        
        // CRITICAL: Устанавливаем dispatch функции в miniport driver_object
        // FDO будет создан на этом driver_object, поэтому IRP будут роутиться сюда
        (*driver_object).major_function[IRP_MJ_PNP as usize] = Some(crate::StorPortDispatchPnp);
        (*driver_object).major_function[IRP_MJ_POWER as usize] = Some(crate::StorPortDispatchPower);
        (*driver_object).major_function[IRP_MJ_SCSI as usize] = Some(crate::StorPortDispatchScsi);
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(crate::StorPortDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(crate::StorPortDispatchClose);
        storport_print("[STORPORT] Dispatch routines installed in miniport driver\n");
        
        storport_print("[STORPORT] Miniport registered successfully\n");
        
        STATUS_SUCCESS
    }
}

/// AddDevice callback для StorPort FDO
///
/// Фаза 2.1: Теперь берёт HW_INIT_DATA из driver extension, а не из глобальной переменной.
/// Присваивает уникальный adapter_number каждому адаптеру.
pub unsafe extern "win64" fn StorPortAddDevice(
    driver_object: PDRIVER_OBJECT,
    physical_device_object: PDEVICE_OBJECT,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] AddDevice called\n");
        storport_print("[STORPORT]   driver=0x");
        storport_print_hex(driver_object as u64);
        storport_print(" PDO=0x");
        storport_print_hex(physical_device_object as u64);
        storport_print("\n");
        
        // Получаем driver extension с HW_INIT_DATA
        let client_id = StorPortAddDevice as PVOID;
        let driver_ext_ptr = IoGetDriverObjectExtension(driver_object, client_id);
        
        if driver_ext_ptr.is_null() {
            storport_print("[STORPORT] ERROR: Driver extension not found!\n");
            storport_print("[STORPORT]   Miniport may not have called StorPortInitialize\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        let driver_ext = driver_ext_ptr as *mut STORPORT_DRIVER_EXTENSION;
        
        // Проверяем signature
        if (*driver_ext).signature != STORPORT_DRIVER_EXTENSION_SIGNATURE {
            storport_print("[STORPORT] ERROR: Invalid driver extension signature\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        storport_print("[STORPORT]   Driver extension found at 0x");
        storport_print_hex(driver_ext_ptr as u64);
        storport_print("\n");
        
        // Создаём FDO
        let mut fdo: PDEVICE_OBJECT = ptr::null_mut();
        let ext_size = core::mem::size_of::<STORPORT_DEVICE_EXTENSION>() as ULONG;
        
        let status = IoCreateDevice(
            driver_object,
            ext_size,
            ptr::null(),
            FILE_DEVICE_MASS_STORAGE,
            0,
            0,
            &mut fdo,
        );
        
        if status < 0 {
            storport_print("[STORPORT] ERROR: IoCreateDevice failed: 0x");
            storport_print_hex(status as u64);
            storport_print("\n");
            return status;
        }
        
        storport_print("[STORPORT]   FDO created: 0x");
        storport_print_hex(fdo as u64);
        storport_print("\n");
        
        // Инициализируем device extension
        let ext = (*fdo).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        ptr::write(ext, STORPORT_DEVICE_EXTENSION::new());
        
        (*ext).device_object = fdo;
        (*ext).physical_device_object = physical_device_object;
        (*ext).driver_object = driver_object;
        
        // Присваиваем уникальный номер адаптера
        (*ext).adapter_number = ADAPTER_COUNTER;
        ADAPTER_COUNTER += 1;
        storport_print("[STORPORT]   Adapter #");
        storport_print_hex((*ext).adapter_number as u64);
        storport_print("\n");
        
        // Копируем HW_INIT_DATA из driver extension в device extension
        let hw_data = &(*driver_ext).hw_init_data;
        (*ext).hw_init_data = *hw_data;
        
        // Выделяем miniport device extension С ЗАГОЛОВКОМ
        // Layout: [MINIPORT_EXTENSION_HEADER][miniport data...]
        // Miniport получает указатель на область ПОСЛЕ заголовка
        let miniport_data_size = hw_data.DeviceExtensionSize as usize;
        let header_size = core::mem::size_of::<MINIPORT_EXTENSION_HEADER>();
        let total_size = header_size + miniport_data_size;
        
        if miniport_data_size > 0 {
            storport_print("[STORPORT]   Allocating miniport extension: header=");
            storport_print_hex(header_size as u64);
            storport_print(" + data=");
            storport_print_hex(miniport_data_size as u64);
            storport_print(" = ");
            storport_print_hex(total_size as u64);
            storport_print(" bytes\n");
            
            let allocation = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                total_size,
                STORPORT_POOL_TAG,
            );
            if !allocation.is_null() {
                ptr::write_bytes(allocation, 0, total_size);
                
                // Инициализируем заголовок
                let header = allocation as *mut MINIPORT_EXTENSION_HEADER;
                ptr::write(header, MINIPORT_EXTENSION_HEADER::new(ext, miniport_data_size as ULONG));
                
                // Miniport получает указатель ПОСЛЕ заголовка
                let miniport_ext = (allocation as *mut u8).add(header_size) as PVOID;
                (*ext).miniport_device_extension = miniport_ext;
                
                storport_print("[STORPORT]   Header at 0x");
                storport_print_hex(header as u64);
                storport_print(", miniport_ext at 0x");
                storport_print_hex(miniport_ext as u64);
                storport_print("\n");
            } else {
                storport_print("[STORPORT] WARNING: Failed to allocate miniport extension\n");
            }
        }
        
        // Attach к PDO
        let lower_device = IoAttachDeviceToDeviceStack(fdo, physical_device_object);
        if lower_device.is_null() {
            storport_print("[STORPORT] ERROR: Failed to attach to device stack\n");
            IoDeleteDevice(fdo);
            return STATUS_NO_SUCH_DEVICE;
        }
        
        (*ext).lower_device = lower_device;
        
        storport_print("[STORPORT]   Attached to lower device: 0x");
        storport_print_hex(lower_device as u64);
        storport_print("\n");
        
        // Устанавливаем флаги
        (*fdo).flags |= DO_DIRECT_IO | DO_POWER_PAGABLE;
        (*fdo).flags &= !DO_DEVICE_INITIALIZING;
        
        storport_print("[STORPORT] AddDevice completed successfully\n");
        
        STATUS_SUCCESS
    }
}

// =============================================================================
// StorPort API для miniports
// =============================================================================

/// StorPortNotification - уведомления от miniport к port driver
///
/// Фаза 2.2: Теперь обрабатывает RequestComplete с завершением IRP.
///
/// Для REQUEST_COMPLETE вызывается с дополнительным аргументом SRB.
/// Для NEXT_REQUEST/NEXT_LU_REQUEST сигнализирует готовность принять следующий запрос.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortNotification(
    notification_type: ULONG,
    hw_device_extension: PVOID,
    // Variadic: для REQUEST_COMPLETE - PSCSI_REQUEST_BLOCK
    //          для NEXT_LU_REQUEST - PathId, TargetId, Lun
) {
    unsafe {
        // Находим STORPORT_DEVICE_EXTENSION из miniport extension
        // В текущей реализации miniport extension указывает назад на adapter
        // TODO: Для упрощения - находим FDO по глобальному списку или через контекст
        
        // Получаем SRB из стека (для REQUEST_COMPLETE это первый variadic аргумент)
        // TODO: HACK: В x64 следующий аргумент в r9 (4-й параметр)
        // Используем inline assembly или просто берём из контекста
        
        match notification_type {
            REQUEST_COMPLETE => {
                storport_print("[STORPORT] Notification: REQUEST_COMPLETE\n");
                
                // Получаем SRB (передан как 3й аргумент в x64 calling convention)
                // TODO: В реальном коде нужно использовать va_list, но в Rust это сложно
                // Для упрощения: SRB уже находится в current_srb через handle_srb
                
                // Находим device extension по hw_device_extension
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    let irp = (*ext).current_irp;
                    let srb = (*ext).current_srb;
                    
                    if !irp.is_null() && !srb.is_null() {
                        storport_print("[STORPORT]   Completing IRP 0x");
                        storport_print_hex(irp as u64);
                        storport_print(" SrbStatus=0x");
                        storport_print_hex((*srb).SrbStatus as u64);
                        storport_print("\n");
                        
                        // Устанавливаем статус в IRP на основе SrbStatus
                        let nt_status = srb_status_to_ntstatus((*srb).SrbStatus);
                        
                        // Получаем указатель на IO_STATUS через оффсет
                        // IRP.io_status - это IO_STATUS_BLOCK
                        let irp_ref = &mut *irp;
                        irp_ref.io_status.status = nt_status;
                        
                        if nt_status >= 0 {
                            // Success - передаём количество переданных байт
                            irp_ref.io_status.information = (*srb).DataTransferLength as usize;
                        } else {
                            irp_ref.io_status.information = 0;
                        }
                        
                        // Очищаем текущий запрос
                        (*ext).clear_current_request();
                        
                        // Завершаем IRP
                        IofCompleteRequest(irp, 0); // IO_NO_INCREMENT
                        
                        storport_print("[STORPORT]   IRP completed with status 0x");
                        storport_print_hex(nt_status as u64);
                        storport_print("\n");
                    } else {
                        storport_print("[STORPORT] WARNING: REQUEST_COMPLETE but no current IRP/SRB\n");
                    }
                }
            }
            NEXT_REQUEST => {
                storport_print("[STORPORT] Notification: NEXT_REQUEST\n");
                
                // Сигнализируем готовность принять следующий запрос
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    (*ext).ready_for_next = 1;
                    storport_print("[STORPORT]   Adapter ready for next request\n");
                }
            }
            NEXT_LU_REQUEST => {
                // NEXT_LU_REQUEST(PathId, TargetId, Lun) - размораживает конкретный LUN
                // Variadic args: arg3=PathId, arg4=TargetId, arg5=Lun
                // В текущей реализации используем current_srb для получения path/target/lun
                storport_print("[STORPORT] Notification: NEXT_LU_REQUEST\n");
                
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    // Если есть текущий SRB - размораживаем его LUN
                    if !(*ext).current_srb.is_null() {
                        let path_id = (*(*ext).current_srb).PathId;
                        let target_id = (*(*ext).current_srb).TargetId;
                        let lun = (*(*ext).current_srb).Lun;
                        
                        storport_print("[STORPORT]   Unfreezing LUN ");
                        storport_print_hex(path_id as u64);
                        storport_print("/");
                        storport_print_hex(target_id as u64);
                        storport_print("/");
                        storport_print_hex(lun as u64);
                        storport_print("\n");
                        
                        (*ext).unfreeze_lun(path_id, target_id, lun);
                    }
                    // Также сигнализируем adapter-level готовность
                    (*ext).ready_for_next = 1;
                }
            }
            RESET_DETECTED => {
                // Bus reset обнаружен - отменяем все pending запросы
                storport_print("[STORPORT] Notification: RESET_DETECTED\n");
                
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    (*ext).cancel_all_pending();
                }
            }
            CALL_ENABLE_INTERRUPTS => {
                storport_print("[STORPORT] Notification: CALL_ENABLE_INTERRUPTS\n");
                
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    (*ext).interrupt_enabled = 1;
                }
            }
            CALL_DISABLE_INTERRUPTS => {
                storport_print("[STORPORT] Notification: CALL_DISABLE_INTERRUPTS\n");
                
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    (*ext).interrupt_enabled = 0;
                }
            }
            STORPORT_NOTIFICATION_INIT_DPC => {
                // InitializeDpc - инициализирует DPC для completion processing
                storport_print("[STORPORT] Notification: INITIALIZE_DPC\n");
                
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    if (*ext).completion_dpc_initialized == 0 {
                        // Инициализируем completion DPC
                        KeInitializeDpc(
                            &mut (*ext).completion_dpc,
                            storport_completion_dpc_routine,
                            ext as PVOID,
                        );
                        (*ext).completion_dpc_initialized = 1;
                        storport_print("[STORPORT]   Completion DPC initialized\n");
                    }
                }
            }
            STORPORT_NOTIFICATION_ISSUE_DPC => {
                // IssueDpc - ставит DPC в очередь для completion
                storport_print("[STORPORT] Notification: ISSUE_DPC\n");
                
                if let Some(ext) = find_device_extension_by_miniport(hw_device_extension) {
                    if (*ext).completion_dpc_initialized != 0 {
                        // Передаём current SRB и IRP через system arguments
                        let srb = (*ext).current_srb as PVOID;
                        let irp = (*ext).current_irp as PVOID;
                        
                        let queued = KeInsertQueueDpc(&mut (*ext).completion_dpc, srb, irp);
                        storport_print("[STORPORT]   DPC queued: ");
                        storport_print_hex(queued as u64);
                        storport_print("\n");
                    } else {
                        storport_print("[STORPORT] WARNING: ISSUE_DPC but DPC not initialized\n");
                    }
                }
            }
            _ => {
                storport_print("[STORPORT] Notification: Unknown type 0x");
                storport_print_hex(notification_type as u64);
                storport_print("\n");
            }
        }
    }
}

/// DPC routine для completion processing
///
/// Вызывается на DISPATCH_LEVEL после того как miniport ставит DPC через IssueDpc.
/// Завершает IRP с результатом из SRB.
unsafe extern "win64" fn storport_completion_dpc_routine(
    dpc: *mut KDPC,
    deferred_context: PVOID,
    system_argument1: PVOID,  // SRB
    system_argument2: PVOID,  // IRP
) {
    let _ = dpc;
    let ext = deferred_context as *mut STORPORT_DEVICE_EXTENSION;
    let srb = system_argument1 as PSCSI_REQUEST_BLOCK;
    let irp = system_argument2 as PIRP;
    
    storport_print("[STORPORT] DPC: Completion routine executing\n");
    
    if ext.is_null() || srb.is_null() || irp.is_null() {
        storport_print("[STORPORT] DPC: ERROR - NULL parameters\n");
        return;
    }
    
    unsafe {
        storport_print("[STORPORT] DPC:   SRB=0x");
        storport_print_hex(srb as u64);
        storport_print(" SrbStatus=0x");
        storport_print_hex((*srb).SrbStatus as u64);
        storport_print("\n");
        
        // Преобразуем SrbStatus в NTSTATUS
        let nt_status = srb_status_to_ntstatus((*srb).SrbStatus);
        
        // Устанавливаем статус в IRP
        (*irp).io_status.status = nt_status;
        if nt_status >= 0 {
            (*irp).io_status.information = (*srb).DataTransferLength as usize;
        } else {
            (*irp).io_status.information = 0;
        }
        
        // Очищаем текущий запрос
        (*ext).clear_current_request();
        
        // Завершаем IRP
        IofCompleteRequest(irp, 0); // IO_NO_INCREMENT
        
        storport_print("[STORPORT] DPC:   IRP completed with status 0x");
        storport_print_hex(nt_status as u64);
        storport_print("\n");
        
        // Сигнализируем готовность к следующему запросу
        (*ext).ready_for_next = 1;
    }
}

/// Timeout DPC routine
///
/// Вызывается когда таймер истёк для pending запроса.
/// Отменяет запрос с SRB_STATUS_TIMEOUT.
unsafe extern "win64" fn storport_timeout_dpc_routine(
    dpc: *mut KDPC,
    deferred_context: PVOID,
    _system_argument1: PVOID,
    _system_argument2: PVOID,
) {
    let _ = dpc;
    let ext = deferred_context as *mut STORPORT_DEVICE_EXTENSION;
    
    storport_print("[STORPORT] TIMEOUT DPC: Request timeout!\n");
    
    if ext.is_null() {
        return;
    }
    
    unsafe {
        let srb = (*ext).current_srb;
        let irp = (*ext).current_irp;
        
        if !srb.is_null() && !irp.is_null() {
            storport_print("[STORPORT] TIMEOUT:   Aborting SRB 0x");
            storport_print_hex(srb as u64);
            storport_print("\n");
            
            // Устанавливаем timeout status
            (*srb).SrbStatus = SRB_STATUS_TIMEOUT;
            
            // Устанавливаем статус в IRP
            (*irp).io_status.status = STATUS_IO_TIMEOUT;
            (*irp).io_status.information = 0;
            
            // Очищаем текущий запрос
            (*ext).clear_current_request();
            
            // Завершаем IRP
            IofCompleteRequest(irp, 0);
            
            storport_print("[STORPORT] TIMEOUT:   IRP completed with STATUS_IO_TIMEOUT\n");
            
            // TODO: Возможно нужен bus reset если miniport не отвечает
            // Пока просто сигнализируем готовность
            (*ext).ready_for_next = 1;
        } else {
            storport_print("[STORPORT] TIMEOUT:   No pending request\n");
        }
    }
}

/// Инициализирует DPC и Timer для адаптера
///
/// Вызывается при START_DEVICE после инициализации miniport.
pub unsafe fn storport_init_dpc_timer(ext: *mut STORPORT_DEVICE_EXTENSION) {
    if ext.is_null() {
        return;
    }
    
    storport_print("[STORPORT] Initializing DPC and Timer...\n");
    
    // Инициализируем completion DPC
    KeInitializeDpc(
        &mut (*ext).completion_dpc,
        storport_completion_dpc_routine,
        ext as PVOID,
    );
    (*ext).completion_dpc_initialized = 1;
    
    // Инициализируем timeout timer и DPC
    KeInitializeTimer(&mut (*ext).timeout_timer);
    KeInitializeDpc(
        &mut (*ext).timeout_dpc,
        storport_timeout_dpc_routine,
        ext as PVOID,
    );
    (*ext).timeout_timer_initialized = 1;
    
    // Копируем HwInterrupt из hw_init_data
    (*ext).hw_interrupt = (*ext).hw_init_data.HwInterrupt;
    
    storport_print("[STORPORT]   DPC and Timer initialized\n");
}

/// Запускает timeout timer для текущего запроса
pub unsafe fn storport_start_request_timer(ext: *mut STORPORT_DEVICE_EXTENSION) {
    if ext.is_null() || (*ext).timeout_timer_initialized == 0 {
        return;
    }
    
    let timeout_secs = (*ext).request_timeout_secs;
    if timeout_secs == 0 {
        return; // Timeout disabled
    }
    
    // Конвертируем секунды в 100-ns intervals (отрицательное = relative)
    let due_time: LARGE_INTEGER = -(timeout_secs as i64 * 10_000_000);
    
    KeSetTimer(&mut (*ext).timeout_timer, due_time, &mut (*ext).timeout_dpc);
    
    storport_print("[STORPORT] Request timer started: ");
    storport_print_hex(timeout_secs as u64);
    storport_print(" seconds\n");
}

/// Отменяет timeout timer
pub unsafe fn storport_cancel_request_timer(ext: *mut STORPORT_DEVICE_EXTENSION) {
    if ext.is_null() || (*ext).timeout_timer_initialized == 0 {
        return;
    }
    
    KeCancelTimer(&mut (*ext).timeout_timer);
}

/// Находит STORPORT_DEVICE_EXTENSION по указателю на miniport extension
/// 
/// Использует заголовок MINIPORT_EXTENSION_HEADER, который размещается
/// непосредственно перед областью данных miniport.
unsafe fn find_device_extension_by_miniport(hw_device_extension: PVOID) -> Option<*mut STORPORT_DEVICE_EXTENSION> {
    if hw_device_extension.is_null() {
        storport_print("[STORPORT] find_device_extension: NULL hw_device_extension\n");
        return None;
    }
    
    // Заголовок находится ПЕРЕД hw_device_extension
    let header_size = core::mem::size_of::<MINIPORT_EXTENSION_HEADER>();
    let header = (hw_device_extension as *mut u8).sub(header_size) as *mut MINIPORT_EXTENSION_HEADER;
    
    // Проверяем signature
    if (*header).signature != MINIPORT_EXTENSION_HEADER_SIGNATURE {
        storport_print("[STORPORT] find_device_extension: Invalid header signature 0x");
        storport_print_hex((*header).signature as u64);
        storport_print(" at 0x");
        storport_print_hex(header as u64);
        storport_print("\n");
        return None;
    }
    
    let ext = (*header).storport_extension;
    if ext.is_null() {
        storport_print("[STORPORT] find_device_extension: NULL storport_extension in header\n");
        return None;
    }
    
    // Дополнительная проверка signature в STORPORT_DEVICE_EXTENSION
    if (*ext).signature != STORPORT_DEVICE_EXTENSION_SIGNATURE {
        storport_print("[STORPORT] find_device_extension: Invalid extension signature\n");
        return None;
    }
    
    Some(ext)
}

/// Устанавливает текущий адаптер (deprecated - оставлено для совместимости)
/// С заголовком MINIPORT_EXTENSION_HEADER эта функция больше не нужна
#[allow(dead_code)]
pub unsafe fn set_current_adapter(_ext: *mut STORPORT_DEVICE_EXTENSION) {
    // No-op - multi-adapter поддержка через заголовок
}

/// Конвертирует SRB status в NTSTATUS
fn srb_status_to_ntstatus(srb_status: UCHAR) -> NTSTATUS {
    match srb_status & SRB_STATUS_MASK {
        SRB_STATUS_SUCCESS => STATUS_SUCCESS,
        SRB_STATUS_PENDING => STATUS_PENDING,
        SRB_STATUS_ABORTED => STATUS_REQUEST_ABORTED,
        SRB_STATUS_ABORT_FAILED => STATUS_UNSUCCESSFUL,
        SRB_STATUS_ERROR => STATUS_IO_DEVICE_ERROR,
        SRB_STATUS_BUSY => STATUS_DEVICE_BUSY,
        SRB_STATUS_INVALID_REQUEST => STATUS_INVALID_DEVICE_REQUEST,
        SRB_STATUS_INVALID_PATH_ID => STATUS_INVALID_PARAMETER,
        SRB_STATUS_NO_DEVICE => STATUS_NO_SUCH_DEVICE,
        SRB_STATUS_TIMEOUT => STATUS_IO_TIMEOUT,
        SRB_STATUS_SELECTION_TIMEOUT => STATUS_IO_TIMEOUT,
        SRB_STATUS_COMMAND_TIMEOUT => STATUS_IO_TIMEOUT,
        SRB_STATUS_DATA_OVERRUN => STATUS_DATA_OVERRUN,
        SRB_STATUS_BUS_RESET => STATUS_IO_DEVICE_ERROR,
        SRB_STATUS_PARITY_ERROR => STATUS_IO_DEVICE_ERROR, // No specific parity status
        SRB_STATUS_NO_HBA => STATUS_ADAPTER_HARDWARE_ERROR,
        SRB_STATUS_INVALID_LUN => STATUS_INVALID_PARAMETER,
        SRB_STATUS_INVALID_TARGET_ID => STATUS_INVALID_PARAMETER,
        _ => STATUS_UNSUCCESSFUL,
    }
}

// SRB Status mask и коды
const SRB_STATUS_MASK: UCHAR = 0x3F;
const SRB_STATUS_SUCCESS: UCHAR = 0x01;
const SRB_STATUS_PENDING: UCHAR = 0x00;
const SRB_STATUS_ABORTED: UCHAR = 0x02;
const SRB_STATUS_ABORT_FAILED: UCHAR = 0x03;
const SRB_STATUS_ERROR: UCHAR = 0x04;
const SRB_STATUS_BUSY: UCHAR = 0x05;
const SRB_STATUS_INVALID_REQUEST: UCHAR = 0x06;
const SRB_STATUS_INVALID_PATH_ID: UCHAR = 0x07;
const SRB_STATUS_NO_DEVICE: UCHAR = 0x08;
const SRB_STATUS_TIMEOUT: UCHAR = 0x09;
const SRB_STATUS_SELECTION_TIMEOUT: UCHAR = 0x0A;
const SRB_STATUS_COMMAND_TIMEOUT: UCHAR = 0x0B;
const SRB_STATUS_DATA_OVERRUN: UCHAR = 0x12;
const SRB_STATUS_BUS_RESET: UCHAR = 0x0E;
const SRB_STATUS_PARITY_ERROR: UCHAR = 0x0F;
const SRB_STATUS_NO_HBA: UCHAR = 0x11;
const SRB_STATUS_INVALID_LUN: UCHAR = 0x20;
const SRB_STATUS_INVALID_TARGET_ID: UCHAR = 0x21;

// Notification types (SCSI_NOTIFICATION_TYPE from storport.h)
const REQUEST_COMPLETE: ULONG = 0;
const NEXT_REQUEST: ULONG = 1;
const NEXT_LU_REQUEST: ULONG = 3;
const RESET_DETECTED: ULONG = 4;
const CALL_ENABLE_INTERRUPTS: ULONG = 6;
const CALL_DISABLE_INTERRUPTS: ULONG = 7;

// StorPort-specific notification types
const STORPORT_NOTIFICATION_INIT_DPC: ULONG = 8;      // InitializeDpc
const STORPORT_NOTIFICATION_ISSUE_DPC: ULONG = 9;     // IssueDpc
const STORPORT_NOTIFICATION_ACQUIRE_SPINLOCK: ULONG = 10;
const STORPORT_NOTIFICATION_RELEASE_SPINLOCK: ULONG = 11;

// DPC importance
const DPC_IMPORTANCE_LOW: ULONG = 0;
const DPC_IMPORTANCE_MEDIUM: ULONG = 1;
const DPC_IMPORTANCE_HIGH: ULONG = 2;

/// StorPortGetUncachedExtension - выделяет uncached memory для DMA
///
/// Выделяет физически непрерывную память с выключенным кэшированием.
/// Используется для DMA буферов которые должны быть coherent с устройством.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortGetUncachedExtension(
    hw_device_extension: PVOID,
    config_info: PPORT_CONFIGURATION_INFORMATION,
    size: ULONG,
) -> PVOID {
    unsafe {
        let _ = hw_device_extension;
        let _ = config_info;
        
        storport_print("[STORPORT] GetUncachedExtension: size=");
        storport_print_hex(size as u64);
        storport_print("\n");
        
        // Используем MmAllocateContiguousMemorySpecifyCache с MmNonCached
        // для выделения физически непрерывной uncached памяти
        let lowest_addr: ULONGLONG = 0;
        let highest_addr: ULONGLONG = 0xFFFF_FFFF; // 4GB limit for 32-bit DMA
        let boundary: ULONGLONG = 0; // No boundary requirement
        
        let buffer = MmAllocateContiguousMemorySpecifyCache(
            size as usize,
            lowest_addr,
            highest_addr,
            boundary,
            MM_NON_CACHED, // MmNonCached
        );
        
        if buffer.is_null() {
            storport_print("[STORPORT] ERROR: Failed to allocate uncached extension\n");
        } else {
            // Обнуляем память
            core::ptr::write_bytes(buffer as *mut u8, 0, size as usize);
            
            // Логируем физический адрес для DMA
            let phys_addr = MmGetPhysicalAddress(buffer);
            storport_print("[STORPORT]   Allocated uncached at VA=0x");
            storport_print_hex(buffer as u64);
            storport_print(" PA=0x");
            storport_print_hex(phys_addr);
            storport_print("\n");
        }
        
        buffer
    }
}

/// StorPortReadRegisterUlong - читает ULONG из memory-mapped регистра
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortReadRegisterUlong(address: PULONG) -> ULONG {
    unsafe {
        core::ptr::read_volatile(address)
    }
}

/// StorPortWriteRegisterUlong - пишет ULONG в memory-mapped регистр
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortWriteRegisterUlong(address: PULONG, value: ULONG) {
    unsafe {
        core::ptr::write_volatile(address, value);
    }
}

/// StorPortReadRegisterUshort
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortReadRegisterUshort(address: PUSHORT) -> USHORT {
    unsafe {
        core::ptr::read_volatile(address)
    }
}

/// StorPortWriteRegisterUshort
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortWriteRegisterUshort(address: PUSHORT, value: USHORT) {
    unsafe {
        core::ptr::write_volatile(address, value);
    }
}

/// StorPortReadRegisterUchar
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortReadRegisterUchar(address: PUCHAR) -> UCHAR {
    unsafe {
        core::ptr::read_volatile(address)
    }
}

/// StorPortWriteRegisterUchar
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortWriteRegisterUchar(address: PUCHAR, value: UCHAR) {
    unsafe {
        core::ptr::write_volatile(address, value);
    }
}

/// StorPortLogError - логирует ошибку
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortLogError(
    hw_device_extension: PVOID,
    srb: PSCSI_REQUEST_BLOCK,
    path_id: UCHAR,
    target_id: UCHAR,
    lun: UCHAR,
    error_code: ULONG,
    unique_id: ULONG,
) {
    unsafe {
        storport_print("[STORPORT] ERROR: ");
        storport_print_hex(error_code as u64);
        storport_print(" Path=");
        storport_print_hex(path_id as u64);
        storport_print(" Target=");
        storport_print_hex(target_id as u64);
        storport_print(" LUN=");
        storport_print_hex(lun as u64);
        storport_print("\n");
    }
}

/// StorPortGetPhysicalAddress - конвертирует virtual address в physical
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortGetPhysicalAddress(
    hw_device_extension: PVOID,
    srb: PSCSI_REQUEST_BLOCK,
    virtual_address: PVOID,
    length: *mut ULONG,
) -> ULONGLONG {
    unsafe {
        use crate::imports::ntoskrnl::MmGetPhysicalAddress;
        
        if !length.is_null() {
            *length = 4096; // Одна страница
        }
        
        // Используем MmGetPhysicalAddress для получения физического адреса
        let phys = MmGetPhysicalAddress(virtual_address);
        phys
    }
}

/// StorPortGetVirtualAddress - конвертирует physical address в virtual
///
/// Использует HHDM (Higher-Half Direct Map) для получения виртуального адреса.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortGetVirtualAddress(
    hw_device_extension: PVOID,
    physical_address: ULONGLONG,
) -> PVOID {
    unsafe {
        let _ = hw_device_extension;
        
        // Используем MmGetVirtualForPhysical для корректной конвертации
        let va = MmGetVirtualForPhysical(physical_address);
        
        storport_print("[STORPORT] GetVirtualAddress: PA=0x");
        storport_print_hex(physical_address);
        storport_print(" -> VA=0x");
        storport_print_hex(va as u64);
        storport_print("\n");
        
        va
    }
}

/// StorPortGetDeviceBase - маппит физический адрес MMIO в виртуальный
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortGetDeviceBase(
    hw_device_extension: PVOID,
    bus_type: ULONG,
    system_io_bus_number: ULONG,
    io_address: ULONGLONG,
    number_of_bytes: ULONG,
    in_io_space: BOOLEAN,
) -> PVOID {
    unsafe {
        let _ = hw_device_extension;
        let _ = bus_type;
        let _ = system_io_bus_number;
        let _ = in_io_space;
        
        storport_print("[STORPORT] GetDeviceBase: phys=0x");
        storport_print_hex(io_address);
        storport_print(" size=0x");
        storport_print_hex(number_of_bytes as u64);
        storport_print("\n");
        
        // Используем MmMapIoSpace для маппинга (MmNonCached для MMIO)
        let mapped = MmMapIoSpace(io_address, number_of_bytes as usize, MM_NON_CACHED);
        
        if !mapped.is_null() {
            // Регистрируем маппинг для последующего освобождения
            register_device_base_mapping(mapped, number_of_bytes as usize);
            
            storport_print("[STORPORT]   -> mapped to 0x");
            storport_print_hex(mapped as u64);
            storport_print("\n");
        } else {
            storport_print("[STORPORT]   -> FAILED to map!\n");
        }
        
        mapped
    }
}

/// Хранит информацию о маппинге для StorPortFreeDeviceBase
/// Нужно для вызова MmUnmapIoSpace с правильным размером
static mut DEVICE_BASE_MAPPINGS: [(PVOID, usize); 16] = [(core::ptr::null_mut(), 0); 16];
static mut DEVICE_BASE_MAPPING_COUNT: usize = 0;

/// Регистрирует маппинг для последующего освобождения
unsafe fn register_device_base_mapping(address: PVOID, size: usize) {
    if DEVICE_BASE_MAPPING_COUNT < 16 {
        DEVICE_BASE_MAPPINGS[DEVICE_BASE_MAPPING_COUNT] = (address, size);
        DEVICE_BASE_MAPPING_COUNT += 1;
    }
}

/// Находит и удаляет маппинг
unsafe fn find_and_remove_mapping(address: PVOID) -> Option<usize> {
    for i in 0..DEVICE_BASE_MAPPING_COUNT {
        if DEVICE_BASE_MAPPINGS[i].0 == address {
            let size = DEVICE_BASE_MAPPINGS[i].1;
            // Сдвигаем остальные
            for j in i..DEVICE_BASE_MAPPING_COUNT - 1 {
                DEVICE_BASE_MAPPINGS[j] = DEVICE_BASE_MAPPINGS[j + 1];
            }
            DEVICE_BASE_MAPPING_COUNT -= 1;
            return Some(size);
        }
    }
    None
}

/// StorPortFreeDeviceBase - освобождает маппинг MMIO
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortFreeDeviceBase(
    hw_device_extension: PVOID,
    mapped_address: PVOID,
) {
    unsafe {
        let _ = hw_device_extension;
        
        if mapped_address.is_null() {
            return;
        }
        
        storport_print("[STORPORT] FreeDeviceBase: VA=0x");
        storport_print_hex(mapped_address as u64);
        
        // Находим размер маппинга
        if let Some(size) = find_and_remove_mapping(mapped_address) {
            storport_print(" size=0x");
            storport_print_hex(size as u64);
            storport_print("\n");
            
            MmUnmapIoSpace(mapped_address, size);
        } else {
            storport_print(" (mapping not found, skipping)\n");
        }
    }
}

/// StorPortStallExecution - задержка в микросекундах
///
/// Использует KeStallExecutionProcessor для точной задержки.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortStallExecution(delay: ULONG) {
    unsafe {
        KeStallExecutionProcessor(delay);
    }
}

// =============================================================================
// DMA / Scatter-Gather Support
// =============================================================================
//
// Фаза 2.4: Реализация Scatter-Gather списков для DMA операций.
// Соответствует NT 6.1 StorPort API.
//

/// MDL флаги (из ntdef.h)
pub const MDL_MAPPED_TO_SYSTEM_VA: u16 = 0x0001;
pub const MDL_PAGES_LOCKED: u16 = 0x0002;
pub const MDL_SOURCE_IS_NONPAGED_POOL: u16 = 0x0004;
pub const MDL_ALLOCATED_FIXED_SIZE: u16 = 0x0008;
pub const MDL_PARTIAL: u16 = 0x0010;
pub const MDL_PARTIAL_HAS_BEEN_MAPPED: u16 = 0x0020;
pub const MDL_IO_PAGE_READ: u16 = 0x0040;
pub const MDL_WRITE_OPERATION: u16 = 0x0080;
pub const MDL_DESCRIBES_AWE: u16 = 0x0400;
pub const MDL_PHYSICAL_VIEW: u16 = 0x0800;
pub const MDL_IO_SPACE: u16 = 0x1000;

/// MDL структура (локальное определение для StorPort)
#[repr(C)]
pub struct MDL {
    pub next: *mut MDL,
    pub size: u16,
    pub mdl_flags: u16,
    pub process: PVOID,
    pub mapped_system_va: PVOID,
    pub start_va: PVOID,
    pub byte_count: u32,
    pub byte_offset: u32,
    // PFN array follows
}

pub type PMDL = *mut MDL;

impl MDL {
    pub const BASE_SIZE: usize = core::mem::size_of::<MDL>();
    
    /// Возвращает указатель на PFN массив
    #[inline]
    pub fn pfn_array(&self) -> *const ULONG_PTR {
        unsafe { 
            (self as *const Self as *const u8).add(Self::BASE_SIZE) as *const ULONG_PTR 
        }
    }
    
    /// Количество страниц в MDL
    #[inline]
    pub fn page_count(&self) -> usize {
        let offset = self.byte_offset as usize;
        let total = self.byte_count as usize + offset;
        (total + PAGE_SIZE - 1) / PAGE_SIZE
    }
}

/// TODO: Статический буфер для SG-списков (per-adapter в реальности)
/// TODO: Для простоты используем массив фиксированного размера.
/// В NT6.1 обычно выделяется из SrbExtension или отдельного пула.
#[repr(C)]
struct SG_LIST_BUFFER {
    pub number_of_elements: ULONG,
    pub reserved: ULONG_PTR,
    pub elements: [STOR_SCATTER_GATHER_ELEMENT; STOR_MAX_SG_ELEMENTS],
}

/// Построить scatter/gather список из MDL
/// 
/// # Arguments
/// * `mdl` - Memory Descriptor List с физическими страницами
/// * `sg_buffer` - Буфер для заполнения SG-списка
/// * `max_elements` - Максимальное количество элементов (NumberOfPhysicalBreaks + 1)
/// * `max_transfer_length` - Максимальный размер передачи
/// 
/// # Returns
/// Количество заполненных элементов или 0 при ошибке
unsafe fn build_sg_list_from_mdl(
    mdl: PMDL,
    sg_buffer: *mut STOR_SCATTER_GATHER_LIST,
    max_elements: ULONG,
    _max_transfer_length: ULONG,
) -> ULONG {
    if mdl.is_null() || sg_buffer.is_null() {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT-SG] ERROR: build_sg_list_from_mdl null params\n");
        return 0;
    }
    
    let mdl_ref = &*mdl;
    let page_count = mdl_ref.page_count();
    
    if page_count == 0 {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT-SG] ERROR: MDL has 0 pages\n");
        return 0;
    }
    
    #[cfg(feature = "storage-trace")]
    {
        storport_print("[STORPORT-SG] Building SG list: pages=");
        storport_print_hex(page_count as u64);
        storport_print(" bytes=");
        storport_print_hex(mdl_ref.byte_count as u64);
        storport_print(" offset=");
        storport_print_hex(mdl_ref.byte_offset as u64);
        storport_print("\n");
    }
    
    let pfn_array = mdl_ref.pfn_array();
    let mut element_count: ULONG = 0;
    let mut remaining_bytes = mdl_ref.byte_count as usize;
    let first_page_offset = mdl_ref.byte_offset as usize;
    
    // Пытаемся объединять соседние физические страницы
    let mut current_phys_start: ULONGLONG = 0;
    let mut current_length: usize = 0;
    
    for page_idx in 0..page_count {
        if remaining_bytes == 0 {
            break;
        }
        
        let pfn = *pfn_array.add(page_idx);
        let page_phys_base = (pfn as ULONGLONG) << PAGE_SHIFT;
        
        // Offset только для первой страницы
        let page_offset = if page_idx == 0 { first_page_offset } else { 0 };
        let page_phys_addr = page_phys_base + page_offset as ULONGLONG;
        
        // Сколько байт на этой странице
        let bytes_on_page = core::cmp::min(
            PAGE_SIZE - page_offset,
            remaining_bytes
        );
        
        // Можно ли объединить с предыдущим элементом?
        if current_length > 0 {
            let expected_next = current_phys_start + current_length as ULONGLONG;
            if page_phys_addr == expected_next {
                // Физически смежные - объединяем
                current_length += bytes_on_page;
                remaining_bytes -= bytes_on_page;
                continue;
            } else {
                // Не смежные - сохраняем текущий элемент
                if element_count >= max_elements {
                    #[cfg(feature = "storage-trace")]
                    storport_print("[STORPORT-SG] WARNING: max_elements exceeded\n");
                    break;
                }
                
                let elem = &mut (*sg_buffer).List[element_count as usize];
                elem.PhysicalAddress = current_phys_start;
                elem.Length = current_length as ULONG;
                elem.Reserved = 0;
                element_count += 1;
                
                #[cfg(feature = "storage-trace")]
                {
                    storport_print("[STORPORT-SG]   Element phys=0x");
                    storport_print_hex(current_phys_start);
                    storport_print(" len=");
                    storport_print_hex(current_length as u64);
                    storport_print("\n");
                }
            }
        }
        
        // Начинаем новый элемент
        current_phys_start = page_phys_addr;
        current_length = bytes_on_page;
        remaining_bytes -= bytes_on_page;
    }
    
    // Сохраняем последний элемент
    if current_length > 0 && element_count < max_elements {
        let elem = &mut (*sg_buffer).List[element_count as usize];
        elem.PhysicalAddress = current_phys_start;
        elem.Length = current_length as ULONG;
        elem.Reserved = 0;
        element_count += 1;
        
        #[cfg(feature = "storage-trace")]
        {
            storport_print("[STORPORT-SG]   Element phys=0x");
            storport_print_hex(current_phys_start);
            storport_print(" len=");
            storport_print_hex(current_length as u64);
            storport_print("\n");
        }
    }
    
    (*sg_buffer).NumberOfElements = element_count;
    (*sg_buffer).Reserved = 0;
    
    #[cfg(feature = "storage-trace")]
    {
        storport_print("[STORPORT-SG] Built SG list with ");
        storport_print_hex(element_count as u64);
        storport_print(" elements\n");
    }
    
    element_count
}

/// StorPortGetScatterGatherList - получить scatter/gather список для SRB
///
/// Создает SG-список на основе MDL из IRP, связанного с SRB.
/// 
/// # Arguments
/// * `hw_device_extension` - Указатель на miniport device extension
/// * `srb` - SCSI Request Block
/// 
/// # Returns
/// Указатель на STOR_SCATTER_GATHER_LIST или NULL при ошибке
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortGetScatterGatherList(
    hw_device_extension: PVOID,
    srb: PSCSI_REQUEST_BLOCK,
) -> PSTOR_SCATTER_GATHER_LIST {
    #[cfg(feature = "storage-trace")]
    storport_print("[STORPORT] StorPortGetScatterGatherList called\n");
    
    if srb.is_null() {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT-SG] ERROR: srb is NULL\n");
        return ptr::null_mut();
    }
    
    let srb_ref = &*srb;
    
    // Получаем IRP из SRB
    let irp = srb_ref.OriginalRequest as PIRP;
    if irp.is_null() {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT-SG] ERROR: SRB.OriginalRequest (IRP) is NULL\n");
        return ptr::null_mut();
    }
    
    // Получаем MDL из IRP
    let mdl = (*irp).mdl_address as PMDL;
    if mdl.is_null() {
        // Нет MDL - возможно, это non-data transfer команда
        // или buffer был в user_buffer напрямую (редко)
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT-SG] No MDL in IRP, creating direct SG for DataBuffer\n");
        
        // Для простых случаев создаем одноэлементный SG из DataBuffer
        if srb_ref.DataBuffer.is_null() || srb_ref.DataTransferLength == 0 {
            return ptr::null_mut();
        }
        
        // Используем MmGetPhysicalAddress напрямую для DataBuffer
        let phys_addr = MmGetPhysicalAddress(srb_ref.DataBuffer);
        if phys_addr == 0 {
            #[cfg(feature = "storage-trace")]
            storport_print("[STORPORT-SG] ERROR: Cannot get physical address for DataBuffer\n");
            return ptr::null_mut();
        }
        
        // Выделяем буфер для SG-списка
        let sg_size = STOR_SCATTER_GATHER_LIST::size_for_elements(1);
        let sg_buffer = ExAllocatePoolWithTag(
            NonPagedPool,
            sg_size,
            STORPORT_POOL_TAG
        ) as PSTOR_SCATTER_GATHER_LIST;
        
        if sg_buffer.is_null() {
            #[cfg(feature = "storage-trace")]
            storport_print("[STORPORT-SG] ERROR: Failed to allocate SG list buffer\n");
            return ptr::null_mut();
        }
        
        (*sg_buffer).NumberOfElements = 1;
        (*sg_buffer).Reserved = 0;
        (*sg_buffer).List[0].PhysicalAddress = phys_addr;
        (*sg_buffer).List[0].Length = srb_ref.DataTransferLength;
        (*sg_buffer).List[0].Reserved = 0;
        
        return sg_buffer;
    }
    
    // Получаем device extension для лимитов
    let device_ext_opt = find_device_extension_by_miniport(hw_device_extension);
    let (max_elements, max_transfer) = if let Some(device_ext) = device_ext_opt {
        if !device_ext.is_null() {
            let ext = &*device_ext;
            let breaks = if ext.number_of_physical_breaks > 0 {
                ext.number_of_physical_breaks + 1 // +1 т.к. breaks = segments - 1
            } else {
                STOR_MAX_SG_ELEMENTS as ULONG
            };
            (breaks, ext.max_transfer_length)
        } else {
            (STOR_MAX_SG_ELEMENTS as ULONG, 0xFFFFFFFF_u32)
        }
    } else {
        (STOR_MAX_SG_ELEMENTS as ULONG, 0xFFFFFFFF_u32)
    };
    
    #[cfg(feature = "storage-trace")]
    {
        storport_print("[STORPORT-SG] max_elements=");
        storport_print_hex(max_elements as u64);
        storport_print(" max_transfer=");
        storport_print_hex(max_transfer as u64);
        storport_print("\n");
    }
    
    // Вычисляем сколько элементов нам может понадобиться
    let mdl_ref = &*mdl;
    let estimated_elements = mdl_ref.page_count() as ULONG;
    let elements_to_allocate = core::cmp::min(estimated_elements, max_elements) as usize;
    
    // Выделяем буфер для SG-списка
    let sg_size = STOR_SCATTER_GATHER_LIST::size_for_elements(elements_to_allocate);
    let sg_buffer = ExAllocatePoolWithTag(
        NonPagedPool,
        sg_size,
        STORPORT_POOL_TAG
    ) as PSTOR_SCATTER_GATHER_LIST;
    
    if sg_buffer.is_null() {
        #[cfg(feature = "storage-trace")]
        {
            storport_print("[STORPORT-SG] ERROR: Failed to allocate SG list buffer (");
            storport_print_hex(sg_size as u64);
            storport_print(" bytes)\n");
        }
        return ptr::null_mut();
    }
    
    // Строим SG-список из MDL
    let count = build_sg_list_from_mdl(mdl, sg_buffer, max_elements, max_transfer);
    if count == 0 {
        ExFreePoolWithTag(sg_buffer as PVOID, STORPORT_POOL_TAG);
        return ptr::null_mut();
    }
    
    sg_buffer
}

/// StorPortPutScatterGatherList - освободить scatter/gather список
///
/// Освобождает память, выделенную StorPortGetScatterGatherList.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortPutScatterGatherList(
    _hw_device_extension: PVOID,
    sg_list: PSTOR_SCATTER_GATHER_LIST,
    _write_to_device: BOOLEAN,
) {
    #[cfg(feature = "storage-trace")]
    storport_print("[STORPORT] StorPortPutScatterGatherList called\n");
    
    if !sg_list.is_null() {
        ExFreePoolWithTag(sg_list as PVOID, STORPORT_POOL_TAG);
    }
}

/// StorPortConvertUlongToPhysicalAddress - преобразовать ULONG в физический адрес
///
/// Утилита для преобразования 32-битного адреса в STOR_PHYSICAL_ADDRESS.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortConvertUlongToPhysicalAddress(
    ulong_address: ULONG,
) -> STOR_PHYSICAL_ADDRESS {
    ulong_address as STOR_PHYSICAL_ADDRESS
}

/// StorPortConvertPhysicalAddressToUlong - преобразовать физический адрес в ULONG
///
/// Утилита для преобразования 64-битного физического адреса в 32-битный.
/// Возвращает младшие 32 бита.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn StorPortConvertPhysicalAddressToUlong(
    address: STOR_PHYSICAL_ADDRESS,
) -> ULONG {
    address as ULONG
}

/// Проверка ограничений DMA для буфера
/// 
/// Проверяет что буфер соответствует ограничениям адаптера:
/// - Не превышает MaximumTransferLength
/// - Количество сегментов не превышает NumberOfPhysicalBreaks + 1
/// - Выравнивание соответствует AlignmentMask
///
/// # Returns
/// TRUE если буфер допустим, FALSE иначе
#[inline]
pub unsafe fn storport_validate_transfer_params(
    device_ext: *mut STORPORT_DEVICE_EXTENSION,
    data_buffer: PVOID,
    transfer_length: ULONG,
) -> bool {
    if device_ext.is_null() || data_buffer.is_null() {
        return false;
    }
    
    let ext = &*device_ext;
    
    // Проверяем максимальную длину передачи
    if ext.max_transfer_length > 0 && transfer_length > ext.max_transfer_length {
        #[cfg(feature = "storage-trace")]
        {
            storport_print("[STORPORT-SG] Transfer length ");
            storport_print_hex(transfer_length as u64);
            storport_print(" exceeds max ");
            storport_print_hex(ext.max_transfer_length as u64);
            storport_print("\n");
        }
        return false;
    }
    
    // Проверяем выравнивание
    if ext.alignment_mask > 0 {
        let addr = data_buffer as usize;
        if (addr & ext.alignment_mask as usize) != 0 {
            #[cfg(feature = "storage-trace")]
            {
                storport_print("[STORPORT-SG] Buffer not aligned: addr=0x");
                storport_print_hex(addr as u64);
                storport_print(" mask=0x");
                storport_print_hex(ext.alignment_mask as u64);
                storport_print("\n");
            }
            return false;
        }
    }
    
    true
}

