//! Process Manager Types
//!
//! Базовые типы и константы Process Manager.

use crate::nt::PVOID;
use crate::ob::GENERIC_MAPPING;

// =============================================================================
// Process Access Rights
// =============================================================================

pub const PROCESS_TERMINATE: u32 = 0x0001;
pub const PROCESS_CREATE_THREAD: u32 = 0x0002;
pub const PROCESS_SET_SESSIONID: u32 = 0x0004;
pub const PROCESS_VM_OPERATION: u32 = 0x0008;
pub const PROCESS_VM_READ: u32 = 0x0010;
pub const PROCESS_VM_WRITE: u32 = 0x0020;
pub const PROCESS_DUP_HANDLE: u32 = 0x0040;
pub const PROCESS_CREATE_PROCESS: u32 = 0x0080;
pub const PROCESS_SET_QUOTA: u32 = 0x0100;
pub const PROCESS_SET_INFORMATION: u32 = 0x0200;
pub const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
pub const PROCESS_SUSPEND_RESUME: u32 = 0x0800;
pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

pub const PROCESS_ALL_ACCESS: u32 = 0x001FFFFF;

// =============================================================================
// Thread Access Rights
// =============================================================================

pub const THREAD_TERMINATE: u32 = 0x0001;
pub const THREAD_SUSPEND_RESUME: u32 = 0x0002;
pub const THREAD_GET_CONTEXT: u32 = 0x0008;
pub const THREAD_SET_CONTEXT: u32 = 0x0010;
pub const THREAD_SET_INFORMATION: u32 = 0x0020;
pub const THREAD_QUERY_INFORMATION: u32 = 0x0040;
pub const THREAD_SET_THREAD_TOKEN: u32 = 0x0080;
pub const THREAD_IMPERSONATE: u32 = 0x0100;
pub const THREAD_DIRECT_IMPERSONATION: u32 = 0x0200;
pub const THREAD_ALERT: u32 = 0x0400;

pub const THREAD_ALL_ACCESS: u32 = 0x001FFFFF;

// =============================================================================
// Generic Mappings
// =============================================================================

/// Process generic mapping
pub const PSP_PROCESS_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
    generic_write: PROCESS_CREATE_PROCESS
        | PROCESS_CREATE_THREAD
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_DUP_HANDLE
        | PROCESS_TERMINATE
        | PROCESS_SET_QUOTA
        | PROCESS_SET_INFORMATION
        | PROCESS_SUSPEND_RESUME,
    generic_execute: 0x00100000, // SYNCHRONIZE
    generic_all: PROCESS_ALL_ACCESS,
};

/// Thread generic mapping
pub const PSP_THREAD_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
    generic_write: THREAD_TERMINATE
        | THREAD_SUSPEND_RESUME
        | THREAD_ALERT
        | THREAD_SET_INFORMATION
        | THREAD_SET_CONTEXT,
    generic_execute: 0x00100000, // SYNCHRONIZE
    generic_all: THREAD_ALL_ACCESS,
};

// =============================================================================
// Process Priority Classes
// =============================================================================

pub const PROCESS_PRIORITY_CLASS_UNKNOWN: u8 = 0;
pub const PROCESS_PRIORITY_CLASS_IDLE: u8 = 1;
pub const PROCESS_PRIORITY_CLASS_NORMAL: u8 = 2;
pub const PROCESS_PRIORITY_CLASS_HIGH: u8 = 3;
pub const PROCESS_PRIORITY_CLASS_REALTIME: u8 = 4;
pub const PROCESS_PRIORITY_CLASS_BELOW_NORMAL: u8 = 5;
pub const PROCESS_PRIORITY_CLASS_ABOVE_NORMAL: u8 = 6;

/// Priority table (base priorities for priority classes)
pub const PSP_PRIORITY_TABLE: [i8; 7] = [
    8,  // Unknown -> Normal
    4,  // Idle
    8,  // Normal
    13, // High
    24, // Realtime
    6,  // Below Normal
    10, // Above Normal
];

// =============================================================================
// Process Flags
// =============================================================================

pub const PS_PROCESS_FLAGS_CREATE_REPORTED: u32 = 0x00000001;
pub const PS_PROCESS_FLAGS_NO_DEBUG_INHERIT: u32 = 0x00000002;
pub const PS_PROCESS_FLAGS_PROCESS_EXITING: u32 = 0x00000004;
pub const PS_PROCESS_FLAGS_PROCESS_DELETE: u32 = 0x00000008;
pub const PS_PROCESS_FLAGS_WOW64_SPLIT_PAGES: u32 = 0x00000010;
pub const PS_PROCESS_FLAGS_VM_DELETED: u32 = 0x00000020;
pub const PS_PROCESS_FLAGS_OUTSWAP_ENABLED: u32 = 0x00000040;
pub const PS_PROCESS_FLAGS_OUTSWAPPED: u32 = 0x00000080;
pub const PS_PROCESS_FLAGS_FORK_FAILED: u32 = 0x00000100;
pub const PS_PROCESS_FLAGS_WOW64_4GB_VA_SPACE: u32 = 0x00000200;
pub const PS_PROCESS_FLAGS_ADDRESS_SPACE1: u32 = 0x00000400;
pub const PS_PROCESS_FLAGS_ADDRESS_SPACE2: u32 = 0x00000800;
pub const PS_PROCESS_FLAGS_SET_TIMER_RESOLUTION: u32 = 0x00001000;
pub const PS_PROCESS_FLAGS_BREAK_ON_TERMINATION: u32 = 0x00002000;
pub const PS_PROCESS_FLAGS_CREATING_SESSION: u32 = 0x00004000;
pub const PS_PROCESS_FLAGS_USING_WRITE_WATCH: u32 = 0x00008000;
pub const PS_PROCESS_FLAGS_IN_SESSION: u32 = 0x00010000;
pub const PS_PROCESS_FLAGS_OVERRIDE_ADDRESS_SPACE: u32 = 0x00020000;
pub const PS_PROCESS_FLAGS_HAS_ADDRESS_SPACE: u32 = 0x00040000;
pub const PS_PROCESS_FLAGS_LAUNCH_PREFETCHED: u32 = 0x00080000;
pub const PS_PROCESS_FLAGS_INJECT_INPAGE_ERRORS: u32 = 0x00100000;
pub const PS_PROCESS_FLAGS_VM_TOP_DOWN: u32 = 0x00200000;
pub const PS_PROCESS_FLAGS_IMAGE_NOTIFY_DONE: u32 = 0x00400000;
pub const PS_PROCESS_FLAGS_PDE_UPDATE_NEEDED: u32 = 0x00800000;
pub const PS_PROCESS_FLAGS_VDM_ALLOWED: u32 = 0x01000000;
pub const PS_PROCESS_FLAGS_SMAP_ALLOWED: u32 = 0x02000000;
pub const PS_PROCESS_FLAGS_CREATE_FAILED: u32 = 0x04000000;

// =============================================================================
// Thread Flags
// =============================================================================

pub const PS_CROSS_THREAD_FLAGS_TERMINATED: u32 = 0x00000001;
pub const PS_CROSS_THREAD_FLAGS_DEAD_THREAD: u32 = 0x00000002;
pub const PS_CROSS_THREAD_FLAGS_HIDE_FROM_DEBUGGER: u32 = 0x00000004;
pub const PS_CROSS_THREAD_FLAGS_IMPERSONATING: u32 = 0x00000008;
pub const PS_CROSS_THREAD_FLAGS_SYSTEM: u32 = 0x00000010;
pub const PS_CROSS_THREAD_FLAGS_HARD_ERRORS_DISABLED: u32 = 0x00000020;
pub const PS_CROSS_THREAD_FLAGS_BREAK_ON_TERMINATION: u32 = 0x00000040;
pub const PS_CROSS_THREAD_FLAGS_SKIP_CREATION_MSG: u32 = 0x00000080;
pub const PS_CROSS_THREAD_FLAGS_SKIP_TERMINATION_MSG: u32 = 0x00000100;

// =============================================================================
// Special Process/Thread Handles
// =============================================================================

/// NtCurrentProcess pseudo-handle
pub const NT_CURRENT_PROCESS: isize = -1;
/// NtCurrentThread pseudo-handle
pub const NT_CURRENT_THREAD: isize = -2;

// =============================================================================
// Client ID
// =============================================================================

/// Client ID - уникальный идентификатор потока в системе
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CLIENT_ID {
    /// Идентификатор процесса
    pub unique_process: PVOID,
    /// Идентификатор потока
    pub unique_thread: PVOID,
}

impl CLIENT_ID {
    pub const fn new() -> Self {
        Self {
            unique_process: core::ptr::null_mut(),
            unique_thread: core::ptr::null_mut(),
        }
    }

    pub fn from_ids(process_id: usize, thread_id: usize) -> Self {
        Self {
            unique_process: process_id as PVOID,
            unique_thread: thread_id as PVOID,
        }
    }

    pub fn process_id(&self) -> usize {
        self.unique_process as usize
    }

    pub fn thread_id(&self) -> usize {
        self.unique_thread as usize
    }
}

// =============================================================================
// Process/Thread Creation Parameters
// =============================================================================

/// Тип start routine для системных потоков
pub type PKSTART_ROUTINE = Option<unsafe extern "win64" fn(start_context: PVOID)>;

/// Параметры создания процесса
#[repr(C)]
pub struct PS_CREATE_INFO {
    pub size: usize,
    pub state: u32,
    // Additional fields depending on state
}

/// Attribute для создания процесса/потока
#[repr(C)]
pub struct PS_ATTRIBUTE {
    pub attribute: usize,
    pub size: usize,
    pub value: usize,
    pub return_length: *mut usize,
}

/// Список атрибутов
#[repr(C)]
pub struct PS_ATTRIBUTE_LIST {
    pub total_length: usize,
    pub attributes: [PS_ATTRIBUTE; 1], // Variable length
}

// =============================================================================
// Process Information Classes
// =============================================================================

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PROCESSINFOCLASS {
    ProcessBasicInformation = 0,
    ProcessQuotaLimits = 1,
    ProcessIoCounters = 2,
    ProcessVmCounters = 3,
    ProcessTimes = 4,
    ProcessBasePriority = 5,
    ProcessRaisePriority = 6,
    ProcessDebugPort = 7,
    ProcessExceptionPort = 8,
    ProcessAccessToken = 9,
    ProcessLdtInformation = 10,
    ProcessLdtSize = 11,
    ProcessDefaultHardErrorMode = 12,
    ProcessIoPortHandlers = 13,
    ProcessPooledUsageAndLimits = 14,
    ProcessWorkingSetWatch = 15,
    ProcessUserModeIOPL = 16,
    ProcessEnableAlignmentFaultFixup = 17,
    ProcessPriorityClass = 18,
    ProcessWx86Information = 19,
    ProcessHandleCount = 20,
    ProcessAffinityMask = 21,
    ProcessPriorityBoost = 22,
    ProcessDeviceMap = 23,
    ProcessSessionInformation = 24,
    ProcessForegroundInformation = 25,
    ProcessWow64Information = 26,
    ProcessImageFileName = 27,
    ProcessLUIDDeviceMapsEnabled = 28,
    ProcessBreakOnTermination = 29,
    ProcessDebugObjectHandle = 30,
    ProcessDebugFlags = 31,
    ProcessHandleTracing = 32,
    MaxProcessInfoClass = 33,
}

// =============================================================================
// Thread Information Classes
// =============================================================================

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum THREADINFOCLASS {
    ThreadBasicInformation = 0,
    ThreadTimes = 1,
    ThreadPriority = 2,
    ThreadBasePriority = 3,
    ThreadAffinityMask = 4,
    ThreadImpersonationToken = 5,
    ThreadDescriptorTableEntry = 6,
    ThreadEnableAlignmentFaultFixup = 7,
    ThreadEventPair = 8,
    ThreadQuerySetWin32StartAddress = 9,
    ThreadZeroTlsCell = 10,
    ThreadPerformanceCount = 11,
    ThreadAmILastThread = 12,
    ThreadIdealProcessor = 13,
    ThreadPriorityBoost = 14,
    ThreadSetTlsArrayAddress = 15,
    ThreadIsIoPending = 16,
    ThreadHideFromDebugger = 17,
    ThreadBreakOnTermination = 18,
    MaxThreadInfoClass = 19,
}

// =============================================================================
// Process Basic Information
// =============================================================================

#[repr(C)]
pub struct PROCESS_BASIC_INFORMATION {
    pub exit_status: i32,
    pub peb_base_address: PVOID,
    pub affinity_mask: usize,
    pub base_priority: i32,
    pub unique_process_id: usize,
    pub inherited_from_unique_process_id: usize,
}

impl PROCESS_BASIC_INFORMATION {
    pub const fn new() -> Self {
        Self {
            exit_status: 0,
            peb_base_address: core::ptr::null_mut(),
            affinity_mask: 0,
            base_priority: 0,
            unique_process_id: 0,
            inherited_from_unique_process_id: 0,
        }
    }
}

// =============================================================================
// Quantum Configuration
// =============================================================================

/// Fixed quantums (short/long)
pub const PSP_FIXED_QUANTUMS: [u8; 6] = [
    3 * 6,
    3 * 6,
    3 * 6, // Short: Level 1, 2, 3
    6 * 6,
    6 * 6,
    6 * 6, // Long: Level 1, 2, 3
];

/// Variable quantums
pub const PSP_VARIABLE_QUANTUMS: [u8; 6] = [
    1 * 6,
    2 * 6,
    3 * 6, // Short: Level 1, 2, 3
    2 * 6,
    4 * 6,
    6 * 6, // Long: Level 1, 2, 3
];
