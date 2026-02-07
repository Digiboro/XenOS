//! PnP Device Node State Machine
//!
//! Определяет состояния устройств в PnP дереве и флаги devnode.
//!
//! Источники:
//! - ReactOS: sdk/include/ndk/iotypes.h
//! - Windows 7 WDK: wdm.h

use crate::nt::ULONG;

// =============================================================================
// PNP_DEVNODE_STATE — состояния узла устройства
// =============================================================================

/// Количество записей в истории состояний
pub const DEVNODE_HISTORY_SIZE: usize = 20;

/// PNP_DEVNODE_STATE — состояния узла устройства в PnP state machine
///
/// Состояния пронумерованы как в NT (0x300+) для совместимости с отладочными
/// инструментами и дампами.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PnpDevnodeState {
    /// Не определено
    Unspecified = 0x300,
    /// Узел создан, но не инициализирован
    Uninitialized = 0x301,
    /// Узел инициализирован (IDs запрошены)
    Initialized = 0x302,
    /// Драйверы добавлены (AddDevice вызван)
    DriversAdded = 0x303,
    /// Ресурсы назначены
    ResourcesAssigned = 0x304,
    /// Ожидание завершения StartDevice
    StartPending = 0x305,
    /// StartDevice завершается
    StartCompletion = 0x306,
    /// Post-start обработка
    StartPostWork = 0x307,
    /// Устройство запущено и работает
    Started = 0x308,
    /// QueryStop отправлен
    QueryStopped = 0x309,
    /// Устройство остановлено
    Stopped = 0x30a,
    /// Restart завершается
    RestartCompletion = 0x30b,
    /// Ожидание enumeration
    EnumeratePending = 0x30c,
    /// Enumeration завершается
    EnumerateCompletion = 0x30d,
    /// Ожидание start
    AwaitingQueuedDeletion = 0x30e,
    /// Ожидание удаления
    AwaitingQueuedRemoval = 0x30f,
    /// QueryRemove отправлен
    QueryRemoved = 0x310,
    /// Удаление ожидает IRP
    RemovePendingCloses = 0x311,
    /// Удаление запланировано
    Removed = 0x312,
    /// Ожидание закрытия хэндлов после удаления
    DeletePendingCloses = 0x313,
    /// Узел удалён
    Deleted = 0x314,
    /// Максимальное значение состояния
    MaxState = 0x315,
}

impl Default for PnpDevnodeState {
    fn default() -> Self {
        Self::Uninitialized
    }
}

impl PnpDevnodeState {
    /// Проверяет, находится ли устройство в "запущенном" состоянии
    pub fn is_started(self) -> bool {
        matches!(
            self,
            Self::StartPending
                | Self::StartCompletion
                | Self::StartPostWork
                | Self::Started
                | Self::QueryStopped
                | Self::EnumeratePending
                | Self::EnumerateCompletion
        )
    }

    /// Проверяет, можно ли отправлять I/O запросы устройству
    pub fn can_receive_io(self) -> bool {
        matches!(
            self,
            Self::Started | Self::QueryStopped | Self::EnumeratePending | Self::EnumerateCompletion
        )
    }
}

// =============================================================================
// DNF_* — флаги DEVICE_NODE
// =============================================================================

/// Устройство создано вручную (legacy/reported)
pub const DNF_MADEUP: ULONG = 0x00000001;
/// AddDevice был вызван (FDO создан)
pub const DNF_ADDED: ULONG = 0x00000002;
/// Дубликат устройства
pub const DNF_DUPLICATE: ULONG = 0x00000004;
/// HAL устройство
pub const DNF_HAL_NODE: ULONG = 0x00000004;
/// Требуется повторная enumeration
pub const DNF_REENUMERATE: ULONG = 0x00000008;
/// Устройство перечислено (enumerated)
pub const DNF_ENUMERATED: ULONG = 0x00000010;
/// IDs запрошены
pub const DNF_IDS_QUERIED: ULONG = 0x00000020;
/// Есть boot конфигурация
pub const DNF_HAS_BOOT_CONFIG: ULONG = 0x00000040;
/// Boot конфигурация зарезервирована
pub const DNF_BOOT_CONFIG_RESERVED: ULONG = 0x00000080;
/// Ресурсы не требуются
pub const DNF_NO_RESOURCE_REQUIRED: ULONG = 0x00000100;
/// Требования ресурсов нужно отфильтровать
pub const DNF_RESOURCE_REQUIREMENTS_NEED_FILTERED: ULONG = 0x00000200;
/// Требования ресурсов изменились
pub const DNF_RESOURCE_REQUIREMENTS_CHANGED: ULONG = 0x00000400;
/// Non-stopped rebalance
pub const DNF_NON_STOPPED_REBALANCE: ULONG = 0x00000800;
/// Legacy драйвер
pub const DNF_LEGACY_DRIVER: ULONG = 0x00001000;
/// Есть проблема
pub const DNF_HAS_PROBLEM: ULONG = 0x00002000;
/// Есть приватная проблема
pub const DNF_HAS_PRIVATE_PROBLEM: ULONG = 0x00004000;
/// Hardware verification
pub const DNF_HARDWARE_VERIFICATION: ULONG = 0x00008000;
/// Устройство отсутствует (gone)
pub const DNF_DEVICE_GONE: ULONG = 0x00010000;
/// Legacy resource devicenode
pub const DNF_LEGACY_RESOURCE_DEVICENODE: ULONG = 0x00020000;
/// Нужен rebalance
pub const DNF_NEEDS_REBALANCE: ULONG = 0x00040000;
/// Заблокировано для eject
pub const DNF_LOCKED_FOR_EJECT: ULONG = 0x00080000;
/// Драйвер заблокирован
pub const DNF_DRIVER_BLOCKED: ULONG = 0x00100000;
/// Дочернее устройство с невалидным ID
pub const DNF_CHILD_WITH_INVALID_ID: ULONG = 0x00200000;
/// Async start не поддерживается
pub const DNF_ASYNC_START_NOT_SUPPORTED: ULONG = 0x00400000;
/// Async enumeration не поддерживается
pub const DNF_ASYNC_ENUMERATION_NOT_SUPPORTED: ULONG = 0x00800000;
/// Заблокировано для rebalance
pub const DNF_LOCKED_FOR_REBALANCE: ULONG = 0x01000000;
/// Устройство деинсталлировано
pub const DNF_UNINSTALLED: ULONG = 0x02000000;
/// Нет lower device filters
pub const DNF_NO_LOWER_DEVICE_FILTERS: ULONG = 0x04000000;
/// Нет lower class filters
pub const DNF_NO_LOWER_CLASS_FILTERS: ULONG = 0x08000000;
/// Нет service
pub const DNF_NO_SERVICE: ULONG = 0x10000000;
/// Нет upper device filters
pub const DNF_NO_UPPER_DEVICE_FILTERS: ULONG = 0x20000000;
/// Нет upper class filters
pub const DNF_NO_UPPER_CLASS_FILTERS: ULONG = 0x40000000;
/// Ожидание FDO
pub const DNF_WAITING_FOR_FDO: ULONG = 0x80000000;

// =============================================================================
// CM_PROB_* — коды проблем устройства
// =============================================================================

/// Нет проблемы
pub const CM_PROB_NOT_CONFIGURED: ULONG = 0x00000001;
/// Ошибка загрузки драйвера
pub const CM_PROB_DEVLOADER_FAILED: ULONG = 0x00000002;
/// Недостаточно памяти
pub const CM_PROB_OUT_OF_MEMORY: ULONG = 0x00000003;
/// Устройство не запущено
pub const CM_PROB_ENTRY_IS_WRONG_TYPE: ULONG = 0x00000004;
/// Нужны ресурсы
pub const CM_PROB_LACKED_ARBITRATOR: ULONG = 0x00000005;
/// Boot config conflict
pub const CM_PROB_BOOT_CONFIG_CONFLICT: ULONG = 0x00000006;
/// Не удалось отфильтровать ресурсы
pub const CM_PROB_FAILED_FILTER: ULONG = 0x00000007;
/// Devloader не найден
pub const CM_PROB_DEVLOADER_NOT_FOUND: ULONG = 0x00000008;
/// Invalid device ID
pub const CM_PROB_INVALID_DATA: ULONG = 0x00000009;
/// Устройство не запустилось
pub const CM_PROB_FAILED_START: ULONG = 0x0000000A;
/// Liar (устройство врёт о ресурсах)
pub const CM_PROB_LIAR: ULONG = 0x0000000B;
/// Normal conflict
pub const CM_PROB_NORMAL_CONFLICT: ULONG = 0x0000000C;
/// Не идентифицировано
pub const CM_PROB_NOT_VERIFIED: ULONG = 0x0000000D;
/// Нужен перезапуск
pub const CM_PROB_NEED_RESTART: ULONG = 0x0000000E;
/// Reenumeration
pub const CM_PROB_REENUMERATION: ULONG = 0x0000000F;
/// Partial log conf
pub const CM_PROB_PARTIAL_LOG_CONF: ULONG = 0x00000010;
/// Unknown resource type
pub const CM_PROB_UNKNOWN_RESOURCE: ULONG = 0x00000011;
/// Reinstall
pub const CM_PROB_REINSTALL: ULONG = 0x00000012;
/// Registry error
pub const CM_PROB_REGISTRY: ULONG = 0x00000013;
/// VXDLDR error
pub const CM_PROB_VXDLDR: ULONG = 0x00000014;
/// Will be removed
pub const CM_PROB_WILL_BE_REMOVED: ULONG = 0x00000015;
/// Disabled
pub const CM_PROB_DISABLED: ULONG = 0x00000016;
/// Devloader not ready
pub const CM_PROB_DEVLOADER_NOT_READY: ULONG = 0x00000017;
/// Device not there
pub const CM_PROB_DEVICE_NOT_THERE: ULONG = 0x00000018;
/// Moved
pub const CM_PROB_MOVED: ULONG = 0x00000019;
/// Too early
pub const CM_PROB_TOO_EARLY: ULONG = 0x0000001A;
/// No valid log conf
pub const CM_PROB_NO_VALID_LOG_CONF: ULONG = 0x0000001B;
/// Failed install
pub const CM_PROB_FAILED_INSTALL: ULONG = 0x0000001C;
/// Hardware disabled
pub const CM_PROB_HARDWARE_DISABLED: ULONG = 0x0000001D;
/// Can't share IRQ
pub const CM_PROB_CANT_SHARE_IRQ: ULONG = 0x0000001E;
/// Failed add
pub const CM_PROB_FAILED_ADD: ULONG = 0x0000001F;
/// Disabled service
pub const CM_PROB_DISABLED_SERVICE: ULONG = 0x00000020;
/// Translation failed
pub const CM_PROB_TRANSLATION_FAILED: ULONG = 0x00000021;
/// No softconfig
pub const CM_PROB_NO_SOFTCONFIG: ULONG = 0x00000022;
/// Bios table
pub const CM_PROB_BIOS_TABLE: ULONG = 0x00000023;
/// IRQ translation failed
pub const CM_PROB_IRQ_TRANSLATION_FAILED: ULONG = 0x00000024;
/// Failed driver entry
pub const CM_PROB_FAILED_DRIVER_ENTRY: ULONG = 0x00000025;
/// Driver failed prior unload
pub const CM_PROB_DRIVER_FAILED_PRIOR_UNLOAD: ULONG = 0x00000026;
/// Driver failed load
pub const CM_PROB_DRIVER_FAILED_LOAD: ULONG = 0x00000027;
/// Driver service key invalid
pub const CM_PROB_DRIVER_SERVICE_KEY_INVALID: ULONG = 0x00000028;
/// Legacy service no devices
pub const CM_PROB_LEGACY_SERVICE_NO_DEVICES: ULONG = 0x00000029;
/// Duplicate device
pub const CM_PROB_DUPLICATE_DEVICE: ULONG = 0x0000002A;
/// Failed post start
pub const CM_PROB_FAILED_POST_START: ULONG = 0x0000002B;
/// Halted
pub const CM_PROB_HALTED: ULONG = 0x0000002C;
/// Phantom
pub const CM_PROB_PHANTOM: ULONG = 0x0000002D;
/// System shutdown
pub const CM_PROB_SYSTEM_SHUTDOWN: ULONG = 0x0000002E;
/// Held for eject
pub const CM_PROB_HELD_FOR_EJECT: ULONG = 0x0000002F;
/// Driver blocked
pub const CM_PROB_DRIVER_BLOCKED: ULONG = 0x00000030;
/// Registry too large
pub const CM_PROB_REGISTRY_TOO_LARGE: ULONG = 0x00000031;
/// Setproperties failed
pub const CM_PROB_SETPROPERTIES_FAILED: ULONG = 0x00000032;
/// Waiting on dependency
pub const CM_PROB_WAITING_ON_DEPENDENCY: ULONG = 0x00000033;
/// Unsigned driver
pub const CM_PROB_UNSIGNED_DRIVER: ULONG = 0x00000034;

