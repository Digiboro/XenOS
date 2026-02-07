//! Типы безопасности NT (Win7 / NT 6.1)
//!
//! Структуры и константы для SE подсистемы: SID, ACL, ACE, Security Descriptor,
//! Token, привилегии и т.д.
//!
//! Источники:
//! - ReactOS: include/ndk/setypes.h, include/ndk/sefuncs.h
//! - WDK: wdm.h, ntifs.h
//! - NT5: ntsec.h, ntseapi.h

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use super::{ACCESS_MASK, BOOLEAN, LARGE_INTEGER, LONG, PVOID, UCHAR, ULONG, USHORT};

// =============================================================================
// GUID - Globally Unique Identifier
// =============================================================================

/// GUID - глобально уникальный идентификатор (128 бит)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GUID {
    pub data1: ULONG,
    pub data2: USHORT,
    pub data3: USHORT,
    pub data4: [UCHAR; 8],
}

impl GUID {
    pub const fn new(data1: ULONG, data2: USHORT, data3: USHORT, data4: [UCHAR; 8]) -> Self {
        Self { data1, data2, data3, data4 }
    }

    pub const fn null() -> Self {
        Self { data1: 0, data2: 0, data3: 0, data4: [0; 8] }
    }
}

// =============================================================================
// LUID - Locally Unique Identifier
// =============================================================================

/// LUID - локально уникальный идентификатор
///
/// Используется для идентификации привилегий и logon sessions.
/// В отличие от GUID, уникален только в пределах текущей загрузки системы.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LUID {
    pub low_part: ULONG,
    pub high_part: LONG,
}

impl LUID {
    pub const fn new(low: ULONG, high: LONG) -> Self {
        Self {
            low_part: low,
            high_part: high,
        }
    }

    /// Преобразование в u64
    #[inline]
    pub const fn as_u64(&self) -> u64 {
        ((self.high_part as u64) << 32) | (self.low_part as u64)
    }

    /// Создание из u64
    #[inline]
    pub const fn from_u64(value: u64) -> Self {
        Self {
            low_part: value as ULONG,
            high_part: (value >> 32) as LONG,
        }
    }
}

/// LUID_AND_ATTRIBUTES - LUID с атрибутами (для привилегий/групп)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LUID_AND_ATTRIBUTES {
    pub luid: LUID,
    pub attributes: ULONG,
}

impl LUID_AND_ATTRIBUTES {
    pub const fn new(luid: LUID, attributes: ULONG) -> Self {
        Self { luid, attributes }
    }
}

// Атрибуты привилегий (LUID_AND_ATTRIBUTES.attributes)
pub const SE_PRIVILEGE_ENABLED_BY_DEFAULT: ULONG = 0x00000001;
pub const SE_PRIVILEGE_ENABLED: ULONG = 0x00000002;
pub const SE_PRIVILEGE_REMOVED: ULONG = 0x00000004;
pub const SE_PRIVILEGE_USED_FOR_ACCESS: ULONG = 0x80000000;

pub const SE_PRIVILEGE_VALID_ATTRIBUTES: ULONG =
    SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_REMOVED | SE_PRIVILEGE_USED_FOR_ACCESS;

// =============================================================================
// SID - Security Identifier
// =============================================================================

/// SID_IDENTIFIER_AUTHORITY - источник SID (6 байт)
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SID_IDENTIFIER_AUTHORITY {
    pub value: [UCHAR; 6],
}

impl SID_IDENTIFIER_AUTHORITY {
    pub const fn new(value: [UCHAR; 6]) -> Self {
        Self { value }
    }
}

impl Default for SID_IDENTIFIER_AUTHORITY {
    fn default() -> Self {
        Self { value: [0; 6] }
    }
}

// Well-known SID authorities
pub const SECURITY_NULL_SID_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 0] };
pub const SECURITY_WORLD_SID_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 1] };
pub const SECURITY_LOCAL_SID_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 2] };
pub const SECURITY_CREATOR_SID_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 3] };
pub const SECURITY_NON_UNIQUE_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 4] };
pub const SECURITY_NT_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 5] };
pub const SECURITY_MANDATORY_LABEL_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY { value: [0, 0, 0, 0, 0, 16] };

/// SID - Security Identifier
///
/// Уникальный идентификатор пользователя, группы или другого security principal.
/// Переменной длины: минимум 8 байт (header) + 4 байта на каждый SubAuthority.
///
/// Максимум 15 SubAuthorities (SID_MAX_SUB_AUTHORITIES).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SID {
    pub revision: UCHAR,
    pub sub_authority_count: UCHAR,
    pub identifier_authority: SID_IDENTIFIER_AUTHORITY,
    /// SubAuthority[0] - первый элемент массива переменной длины
    /// В реальности массив имеет длину sub_authority_count
    pub sub_authority: [ULONG; 1],
}

impl SID {
    /// Минимальный размер SID (без SubAuthorities, только header)
    pub const MIN_SIZE: usize = 8;

    /// Максимальное количество SubAuthorities
    pub const MAX_SUB_AUTHORITIES: usize = 15;

    /// Максимальный размер SID
    pub const MAX_SIZE: usize = Self::MIN_SIZE + Self::MAX_SUB_AUTHORITIES * 4;

    /// Текущая ревизия SID
    pub const REVISION: UCHAR = 1;

    /// Вычисляет размер SID в байтах
    #[inline]
    pub const fn length_required(sub_authority_count: usize) -> usize {
        Self::MIN_SIZE + sub_authority_count * 4
    }

    /// Возвращает размер данного SID
    #[inline]
    pub fn length(&self) -> usize {
        Self::length_required(self.sub_authority_count as usize)
    }
}

/// SID_AND_ATTRIBUTES - SID с атрибутами (для групп в токене)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SID_AND_ATTRIBUTES {
    pub sid: *mut SID,
    pub attributes: ULONG,
}

impl SID_AND_ATTRIBUTES {
    pub const fn new() -> Self {
        Self {
            sid: core::ptr::null_mut(),
            attributes: 0,
        }
    }
}

impl Default for SID_AND_ATTRIBUTES {
    fn default() -> Self {
        Self::new()
    }
}

// Атрибуты групп (SID_AND_ATTRIBUTES.attributes)
pub const SE_GROUP_MANDATORY: ULONG = 0x00000001;
pub const SE_GROUP_ENABLED_BY_DEFAULT: ULONG = 0x00000002;
pub const SE_GROUP_ENABLED: ULONG = 0x00000004;
pub const SE_GROUP_OWNER: ULONG = 0x00000008;
pub const SE_GROUP_USE_FOR_DENY_ONLY: ULONG = 0x00000010;
pub const SE_GROUP_INTEGRITY: ULONG = 0x00000020;
pub const SE_GROUP_INTEGRITY_ENABLED: ULONG = 0x00000040;
pub const SE_GROUP_LOGON_ID: ULONG = 0xC0000000;
pub const SE_GROUP_RESOURCE: ULONG = 0x20000000;

pub const SE_GROUP_VALID_ATTRIBUTES: ULONG = SE_GROUP_MANDATORY
    | SE_GROUP_ENABLED_BY_DEFAULT
    | SE_GROUP_ENABLED
    | SE_GROUP_OWNER
    | SE_GROUP_USE_FOR_DENY_ONLY
    | SE_GROUP_INTEGRITY
    | SE_GROUP_INTEGRITY_ENABLED
    | SE_GROUP_LOGON_ID
    | SE_GROUP_RESOURCE;

// Well-known SID relative identifiers (RIDs)
pub const SECURITY_NULL_RID: ULONG = 0x00000000;
pub const SECURITY_WORLD_RID: ULONG = 0x00000000;
pub const SECURITY_LOCAL_RID: ULONG = 0x00000000;
pub const SECURITY_CREATOR_OWNER_RID: ULONG = 0x00000000;
pub const SECURITY_CREATOR_GROUP_RID: ULONG = 0x00000001;

// NT Authority SID sub-authorities
pub const SECURITY_DIALUP_RID: ULONG = 0x00000001;
pub const SECURITY_NETWORK_RID: ULONG = 0x00000002;
pub const SECURITY_BATCH_RID: ULONG = 0x00000003;
pub const SECURITY_INTERACTIVE_RID: ULONG = 0x00000004;
pub const SECURITY_LOGON_IDS_RID: ULONG = 0x00000005;
pub const SECURITY_SERVICE_RID: ULONG = 0x00000006;
pub const SECURITY_ANONYMOUS_LOGON_RID: ULONG = 0x00000007;
pub const SECURITY_PROXY_RID: ULONG = 0x00000008;
pub const SECURITY_ENTERPRISE_CONTROLLERS_RID: ULONG = 0x00000009;
pub const SECURITY_SERVER_LOGON_RID: ULONG = SECURITY_ENTERPRISE_CONTROLLERS_RID;
pub const SECURITY_PRINCIPAL_SELF_RID: ULONG = 0x0000000A;
pub const SECURITY_AUTHENTICATED_USER_RID: ULONG = 0x0000000B;
pub const SECURITY_RESTRICTED_CODE_RID: ULONG = 0x0000000C;
pub const SECURITY_TERMINAL_SERVER_RID: ULONG = 0x0000000D;
pub const SECURITY_REMOTE_LOGON_RID: ULONG = 0x0000000E;
pub const SECURITY_THIS_ORGANIZATION_RID: ULONG = 0x0000000F;
pub const SECURITY_LOCAL_SYSTEM_RID: ULONG = 0x00000012;
pub const SECURITY_LOCAL_SERVICE_RID: ULONG = 0x00000013;
pub const SECURITY_NETWORK_SERVICE_RID: ULONG = 0x00000014;

// Builtin domain RIDs
pub const SECURITY_BUILTIN_DOMAIN_RID: ULONG = 0x00000020;
pub const DOMAIN_ALIAS_RID_ADMINS: ULONG = 0x00000220;
pub const DOMAIN_ALIAS_RID_USERS: ULONG = 0x00000221;
pub const DOMAIN_ALIAS_RID_GUESTS: ULONG = 0x00000222;
pub const DOMAIN_ALIAS_RID_POWER_USERS: ULONG = 0x00000223;

// Mandatory integrity levels (Win7)
pub const SECURITY_MANDATORY_UNTRUSTED_RID: ULONG = 0x00000000;
pub const SECURITY_MANDATORY_LOW_RID: ULONG = 0x00001000;
pub const SECURITY_MANDATORY_MEDIUM_RID: ULONG = 0x00002000;
pub const SECURITY_MANDATORY_MEDIUM_PLUS_RID: ULONG = 0x00002100;
pub const SECURITY_MANDATORY_HIGH_RID: ULONG = 0x00003000;
pub const SECURITY_MANDATORY_SYSTEM_RID: ULONG = 0x00004000;
pub const SECURITY_MANDATORY_PROTECTED_PROCESS_RID: ULONG = 0x00005000;

// =============================================================================
// ACL - Access Control List
// =============================================================================

/// ACL - Access Control List
///
/// Заголовок списка контроля доступа. За ним следуют ACE записи.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ACL {
    pub acl_revision: UCHAR,
    pub sbz1: UCHAR,
    pub acl_size: USHORT,
    pub ace_count: USHORT,
    pub sbz2: USHORT,
}

impl ACL {
    pub const REVISION: UCHAR = 2;
    pub const REVISION_DS: UCHAR = 4;

    pub const fn new() -> Self {
        Self {
            acl_revision: Self::REVISION,
            sbz1: 0,
            acl_size: core::mem::size_of::<Self>() as USHORT,
            ace_count: 0,
            sbz2: 0,
        }
    }
}

impl Default for ACL {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ACE - Access Control Entry
// =============================================================================

/// ACE_HEADER - заголовок ACE
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ACE_HEADER {
    pub ace_type: UCHAR,
    pub ace_flags: UCHAR,
    pub ace_size: USHORT,
}

// ACE types
pub const ACCESS_ALLOWED_ACE_TYPE: UCHAR = 0x00;
pub const ACCESS_DENIED_ACE_TYPE: UCHAR = 0x01;
pub const SYSTEM_AUDIT_ACE_TYPE: UCHAR = 0x02;
pub const SYSTEM_ALARM_ACE_TYPE: UCHAR = 0x03;

// Object ACE types (Win2000+)
pub const ACCESS_ALLOWED_OBJECT_ACE_TYPE: UCHAR = 0x05;
pub const ACCESS_DENIED_OBJECT_ACE_TYPE: UCHAR = 0x06;
pub const SYSTEM_AUDIT_OBJECT_ACE_TYPE: UCHAR = 0x07;
pub const SYSTEM_ALARM_OBJECT_ACE_TYPE: UCHAR = 0x08;

// Callback ACE types (Vista+)
pub const ACCESS_ALLOWED_CALLBACK_ACE_TYPE: UCHAR = 0x09;
pub const ACCESS_DENIED_CALLBACK_ACE_TYPE: UCHAR = 0x0A;
pub const ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE: UCHAR = 0x0B;
pub const ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE: UCHAR = 0x0C;
pub const SYSTEM_AUDIT_CALLBACK_ACE_TYPE: UCHAR = 0x0D;
pub const SYSTEM_ALARM_CALLBACK_ACE_TYPE: UCHAR = 0x0E;
pub const SYSTEM_AUDIT_CALLBACK_OBJECT_ACE_TYPE: UCHAR = 0x0F;
pub const SYSTEM_ALARM_CALLBACK_OBJECT_ACE_TYPE: UCHAR = 0x10;

// Mandatory label ACE (Vista+)
pub const SYSTEM_MANDATORY_LABEL_ACE_TYPE: UCHAR = 0x11;

// ACE flags
pub const OBJECT_INHERIT_ACE: UCHAR = 0x01;
pub const CONTAINER_INHERIT_ACE: UCHAR = 0x02;
pub const NO_PROPAGATE_INHERIT_ACE: UCHAR = 0x04;
pub const INHERIT_ONLY_ACE: UCHAR = 0x08;
pub const INHERITED_ACE: UCHAR = 0x10;
pub const VALID_INHERIT_FLAGS: UCHAR = 0x1F;

// Audit ACE flags
pub const SUCCESSFUL_ACCESS_ACE_FLAG: UCHAR = 0x40;
pub const FAILED_ACCESS_ACE_FLAG: UCHAR = 0x80;

/// ACCESS_ALLOWED_ACE - разрешающий ACE
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ACCESS_ALLOWED_ACE {
    pub header: ACE_HEADER,
    pub mask: ACCESS_MASK,
    /// SidStart - начало SID (первый ULONG; реальный SID следует дальше)
    pub sid_start: ULONG,
}

/// ACCESS_DENIED_ACE - запрещающий ACE
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ACCESS_DENIED_ACE {
    pub header: ACE_HEADER,
    pub mask: ACCESS_MASK,
    pub sid_start: ULONG,
}

/// SYSTEM_AUDIT_ACE - ACE аудита
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SYSTEM_AUDIT_ACE {
    pub header: ACE_HEADER,
    pub mask: ACCESS_MASK,
    pub sid_start: ULONG,
}

/// SYSTEM_MANDATORY_LABEL_ACE - обязательный уровень целостности (MIC)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SYSTEM_MANDATORY_LABEL_ACE {
    pub header: ACE_HEADER,
    pub mask: ACCESS_MASK,
    pub sid_start: ULONG,
}

// Mandatory label policy masks
pub const SYSTEM_MANDATORY_LABEL_NO_WRITE_UP: ACCESS_MASK = 0x1;
pub const SYSTEM_MANDATORY_LABEL_NO_READ_UP: ACCESS_MASK = 0x2;
pub const SYSTEM_MANDATORY_LABEL_NO_EXECUTE_UP: ACCESS_MASK = 0x4;
pub const SYSTEM_MANDATORY_LABEL_VALID_MASK: ACCESS_MASK = 0x7;

// =============================================================================
// SECURITY_DESCRIPTOR - Security Descriptor
// =============================================================================

/// SECURITY_DESCRIPTOR_CONTROL - флаги дескриптора безопасности
pub type SECURITY_DESCRIPTOR_CONTROL = USHORT;

pub const SE_OWNER_DEFAULTED: SECURITY_DESCRIPTOR_CONTROL = 0x0001;
pub const SE_GROUP_DEFAULTED: SECURITY_DESCRIPTOR_CONTROL = 0x0002;
pub const SE_DACL_PRESENT: SECURITY_DESCRIPTOR_CONTROL = 0x0004;
pub const SE_DACL_DEFAULTED: SECURITY_DESCRIPTOR_CONTROL = 0x0008;
pub const SE_SACL_PRESENT: SECURITY_DESCRIPTOR_CONTROL = 0x0010;
pub const SE_SACL_DEFAULTED: SECURITY_DESCRIPTOR_CONTROL = 0x0020;
pub const SE_DACL_AUTO_INHERIT_REQ: SECURITY_DESCRIPTOR_CONTROL = 0x0100;
pub const SE_SACL_AUTO_INHERIT_REQ: SECURITY_DESCRIPTOR_CONTROL = 0x0200;
pub const SE_DACL_AUTO_INHERITED: SECURITY_DESCRIPTOR_CONTROL = 0x0400;
pub const SE_SACL_AUTO_INHERITED: SECURITY_DESCRIPTOR_CONTROL = 0x0800;
pub const SE_DACL_PROTECTED: SECURITY_DESCRIPTOR_CONTROL = 0x1000;
pub const SE_SACL_PROTECTED: SECURITY_DESCRIPTOR_CONTROL = 0x2000;
pub const SE_RM_CONTROL_VALID: SECURITY_DESCRIPTOR_CONTROL = 0x4000;
pub const SE_SELF_RELATIVE: SECURITY_DESCRIPTOR_CONTROL = 0x8000;

/// SECURITY_DESCRIPTOR - абсолютный формат
///
/// Содержит указатели на Owner SID, Group SID, DACL и SACL.
/// Используется при работе с SD в памяти.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SECURITY_DESCRIPTOR {
    pub revision: UCHAR,
    pub sbz1: UCHAR,
    pub control: SECURITY_DESCRIPTOR_CONTROL,
    pub owner: *mut SID,
    pub group: *mut SID,
    pub sacl: *mut ACL,
    pub dacl: *mut ACL,
}

impl SECURITY_DESCRIPTOR {
    pub const REVISION: UCHAR = 1;
    pub const MIN_LENGTH: usize = core::mem::size_of::<Self>();

    pub const fn new() -> Self {
        Self {
            revision: Self::REVISION,
            sbz1: 0,
            control: 0,
            owner: core::ptr::null_mut(),
            group: core::ptr::null_mut(),
            sacl: core::ptr::null_mut(),
            dacl: core::ptr::null_mut(),
        }
    }
}

impl Default for SECURITY_DESCRIPTOR {
    fn default() -> Self {
        Self::new()
    }
}

/// SECURITY_DESCRIPTOR_RELATIVE - self-relative формат
///
/// Все данные (Owner, Group, DACL, SACL) хранятся как смещения от начала
/// структуры. Используется для хранения/передачи SD.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SECURITY_DESCRIPTOR_RELATIVE {
    pub revision: UCHAR,
    pub sbz1: UCHAR,
    pub control: SECURITY_DESCRIPTOR_CONTROL,
    pub owner: ULONG,  // Смещение от начала SD до Owner SID
    pub group: ULONG,  // Смещение от начала SD до Group SID
    pub sacl: ULONG,   // Смещение от начала SD до SACL
    pub dacl: ULONG,   // Смещение от начала SD до DACL
}

impl SECURITY_DESCRIPTOR_RELATIVE {
    pub const fn new() -> Self {
        Self {
            revision: SECURITY_DESCRIPTOR::REVISION,
            sbz1: 0,
            control: SE_SELF_RELATIVE,
            owner: 0,
            group: 0,
            sacl: 0,
            dacl: 0,
        }
    }
}

impl Default for SECURITY_DESCRIPTOR_RELATIVE {
    fn default() -> Self {
        Self::new()
    }
}

/// SECURITY_INFORMATION - какие части SD запрашиваются/устанавливаются
pub type SECURITY_INFORMATION = ULONG;

pub const OWNER_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x00000001;
pub const GROUP_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x00000002;
pub const DACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x00000004;
pub const SACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x00000008;
pub const LABEL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x00000010;
pub const UNPROTECTED_SACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x10000000;
pub const UNPROTECTED_DACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x20000000;
pub const PROTECTED_SACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x40000000;
pub const PROTECTED_DACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x80000000;

// =============================================================================
// ACCESS_MASK и стандартные права
// =============================================================================

// Generic rights (будут преобразованы в specific через GENERIC_MAPPING)
pub const GENERIC_READ: ACCESS_MASK = 0x80000000;
pub const GENERIC_WRITE: ACCESS_MASK = 0x40000000;
pub const GENERIC_EXECUTE: ACCESS_MASK = 0x20000000;
pub const GENERIC_ALL: ACCESS_MASK = 0x10000000;

// Standard rights (общие для всех типов объектов)
pub const DELETE: ACCESS_MASK = 0x00010000;
pub const READ_CONTROL: ACCESS_MASK = 0x00020000;
pub const WRITE_DAC: ACCESS_MASK = 0x00040000;
pub const WRITE_OWNER: ACCESS_MASK = 0x00080000;
pub const SYNCHRONIZE: ACCESS_MASK = 0x00100000;

pub const STANDARD_RIGHTS_REQUIRED: ACCESS_MASK = DELETE | READ_CONTROL | WRITE_DAC | WRITE_OWNER;
pub const STANDARD_RIGHTS_READ: ACCESS_MASK = READ_CONTROL;
pub const STANDARD_RIGHTS_WRITE: ACCESS_MASK = READ_CONTROL;
pub const STANDARD_RIGHTS_EXECUTE: ACCESS_MASK = READ_CONTROL;
pub const STANDARD_RIGHTS_ALL: ACCESS_MASK = STANDARD_RIGHTS_REQUIRED | SYNCHRONIZE;

// Special access rights
pub const ACCESS_SYSTEM_SECURITY: ACCESS_MASK = 0x01000000;
pub const MAXIMUM_ALLOWED: ACCESS_MASK = 0x02000000;

// =============================================================================
// PRIVILEGE_SET
// =============================================================================

/// PRIVILEGE_SET - набор привилегий
#[repr(C)]
pub struct PRIVILEGE_SET {
    pub privilege_count: ULONG,
    pub control: ULONG,
    /// Первый элемент массива привилегий (переменной длины)
    pub privilege: [LUID_AND_ATTRIBUTES; 1],
}

pub const PRIVILEGE_SET_ALL_NECESSARY: ULONG = 1;

impl PRIVILEGE_SET {
    /// Вычисляет размер PRIVILEGE_SET для заданного количества привилегий
    pub const fn size_for_count(count: usize) -> usize {
        // Базовый размер (PrivilegeCount + Control) + массив привилегий
        core::mem::size_of::<ULONG>() * 2
            + count * core::mem::size_of::<LUID_AND_ATTRIBUTES>()
    }
}

// =============================================================================
// SECURITY_QUALITY_OF_SERVICE и Impersonation
// =============================================================================

/// SECURITY_IMPERSONATION_LEVEL - уровень имперсонации
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SECURITY_IMPERSONATION_LEVEL {
    SecurityAnonymous = 0,
    SecurityIdentification = 1,
    SecurityImpersonation = 2,
    SecurityDelegation = 3,
}

impl Default for SECURITY_IMPERSONATION_LEVEL {
    fn default() -> Self {
        Self::SecurityImpersonation
    }
}

/// SECURITY_CONTEXT_TRACKING_MODE
pub type SECURITY_CONTEXT_TRACKING_MODE = BOOLEAN;

pub const SECURITY_STATIC_TRACKING: SECURITY_CONTEXT_TRACKING_MODE = 0;
pub const SECURITY_DYNAMIC_TRACKING: SECURITY_CONTEXT_TRACKING_MODE = 1;

/// SECURITY_QUALITY_OF_SERVICE - параметры качества безопасности
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SECURITY_QUALITY_OF_SERVICE {
    pub length: ULONG,
    pub impersonation_level: SECURITY_IMPERSONATION_LEVEL,
    pub context_tracking_mode: SECURITY_CONTEXT_TRACKING_MODE,
    pub effective_only: BOOLEAN,
}

impl SECURITY_QUALITY_OF_SERVICE {
    pub const fn new() -> Self {
        Self {
            length: core::mem::size_of::<Self>() as ULONG,
            impersonation_level: SECURITY_IMPERSONATION_LEVEL::SecurityImpersonation,
            context_tracking_mode: SECURITY_DYNAMIC_TRACKING,
            effective_only: 0,
        }
    }
}

impl Default for SECURITY_QUALITY_OF_SERVICE {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TOKEN types и structures
// =============================================================================

/// TOKEN_TYPE - тип токена
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TOKEN_TYPE {
    TokenPrimary = 1,
    TokenImpersonation = 2,
}

impl Default for TOKEN_TYPE {
    fn default() -> Self {
        Self::TokenPrimary
    }
}

/// TOKEN_ELEVATION_TYPE - тип elevation (UAC, Win7)
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TOKEN_ELEVATION_TYPE {
    TokenElevationTypeDefault = 1,
    TokenElevationTypeFull = 2,
    TokenElevationTypeLimited = 3,
}

impl Default for TOKEN_ELEVATION_TYPE {
    fn default() -> Self {
        Self::TokenElevationTypeDefault
    }
}

/// TOKEN_INFORMATION_CLASS - классы информации о токене (Win7)
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TOKEN_INFORMATION_CLASS {
    TokenUser = 1,
    TokenGroups = 2,
    TokenPrivileges = 3,
    TokenOwner = 4,
    TokenPrimaryGroup = 5,
    TokenDefaultDacl = 6,
    TokenSource = 7,
    TokenType = 8,
    TokenImpersonationLevel = 9,
    TokenStatistics = 10,
    TokenRestrictedSids = 11,
    TokenSessionId = 12,
    TokenGroupsAndPrivileges = 13,
    TokenSessionReference = 14,
    TokenSandBoxInert = 15,
    TokenAuditPolicy = 16,
    TokenOrigin = 17,
    TokenElevationType = 18,
    TokenLinkedToken = 19,
    TokenElevation = 20,
    TokenHasRestrictions = 21,
    TokenAccessInformation = 22,
    TokenVirtualizationAllowed = 23,
    TokenVirtualizationEnabled = 24,
    TokenIntegrityLevel = 25,
    TokenUIAccess = 26,
    TokenMandatoryPolicy = 27,
    TokenLogonSid = 28,
    TokenIsAppContainer = 29, // Win8+
    TokenCapabilities = 30,   // Win8+
    TokenAppContainerSid = 31,// Win8+
    TokenAppContainerNumber = 32, // Win8+
    TokenUserClaimAttributes = 33, // Win8+
    TokenDeviceClaimAttributes = 34, // Win8+
    TokenRestrictedUserClaimAttributes = 35, // Win8+
    TokenRestrictedDeviceClaimAttributes = 36, // Win8+
    TokenDeviceGroups = 37, // Win8+
    TokenRestrictedDeviceGroups = 38, // Win8+
    TokenSecurityAttributes = 39, // Win8+
    TokenIsRestricted = 40, // Win8+
    TokenProcessTrustLevel = 41, // Win8.1+
    MaxTokenInfoClass = 42,
}

// Token access rights
pub const TOKEN_ASSIGN_PRIMARY: ACCESS_MASK = 0x0001;
pub const TOKEN_DUPLICATE: ACCESS_MASK = 0x0002;
pub const TOKEN_IMPERSONATE: ACCESS_MASK = 0x0004;
pub const TOKEN_QUERY: ACCESS_MASK = 0x0008;
pub const TOKEN_QUERY_SOURCE: ACCESS_MASK = 0x0010;
pub const TOKEN_ADJUST_PRIVILEGES: ACCESS_MASK = 0x0020;
pub const TOKEN_ADJUST_GROUPS: ACCESS_MASK = 0x0040;
pub const TOKEN_ADJUST_DEFAULT: ACCESS_MASK = 0x0080;
pub const TOKEN_ADJUST_SESSIONID: ACCESS_MASK = 0x0100;

pub const TOKEN_ALL_ACCESS_P: ACCESS_MASK = STANDARD_RIGHTS_REQUIRED
    | TOKEN_ASSIGN_PRIMARY
    | TOKEN_DUPLICATE
    | TOKEN_IMPERSONATE
    | TOKEN_QUERY
    | TOKEN_QUERY_SOURCE
    | TOKEN_ADJUST_PRIVILEGES
    | TOKEN_ADJUST_GROUPS
    | TOKEN_ADJUST_DEFAULT;

pub const TOKEN_ALL_ACCESS: ACCESS_MASK = TOKEN_ALL_ACCESS_P | TOKEN_ADJUST_SESSIONID;

pub const TOKEN_READ: ACCESS_MASK = STANDARD_RIGHTS_READ | TOKEN_QUERY;
pub const TOKEN_WRITE: ACCESS_MASK =
    STANDARD_RIGHTS_WRITE | TOKEN_ADJUST_PRIVILEGES | TOKEN_ADJUST_GROUPS | TOKEN_ADJUST_DEFAULT;
pub const TOKEN_EXECUTE: ACCESS_MASK = STANDARD_RIGHTS_EXECUTE;

/// TOKEN_SOURCE - источник токена
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_SOURCE {
    pub source_name: [i8; 8],
    pub source_identifier: LUID,
}

impl TOKEN_SOURCE {
    pub const fn new() -> Self {
        Self {
            source_name: [0; 8],
            source_identifier: LUID::new(0, 0),
        }
    }
}

impl Default for TOKEN_SOURCE {
    fn default() -> Self {
        Self::new()
    }
}

/// TOKEN_STATISTICS - статистика токена
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TOKEN_STATISTICS {
    pub token_id: LUID,
    pub authentication_id: LUID,
    pub expiration_time: LARGE_INTEGER,
    pub token_type: TOKEN_TYPE,
    pub impersonation_level: SECURITY_IMPERSONATION_LEVEL,
    pub dynamic_charged: ULONG,
    pub dynamic_available: ULONG,
    pub group_count: ULONG,
    pub privilege_count: ULONG,
    pub modified_id: LUID,
}

impl TOKEN_STATISTICS {
    pub const fn new() -> Self {
        Self {
            token_id: LUID::new(0, 0),
            authentication_id: LUID::new(0, 0),
            expiration_time: LARGE_INTEGER::new(0),
            token_type: TOKEN_TYPE::TokenPrimary,
            impersonation_level: SECURITY_IMPERSONATION_LEVEL::SecurityImpersonation,
            dynamic_charged: 0,
            dynamic_available: 0,
            group_count: 0,
            privilege_count: 0,
            modified_id: LUID::new(0, 0),
        }
    }
}

impl Default for TOKEN_STATISTICS {
    fn default() -> Self {
        Self::new()
    }
}

/// TOKEN_USER - информация о пользователе токена
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_USER {
    pub user: SID_AND_ATTRIBUTES,
}

/// TOKEN_GROUPS - группы токена
#[repr(C)]
pub struct TOKEN_GROUPS {
    pub group_count: ULONG,
    pub groups: [SID_AND_ATTRIBUTES; 1], // Переменной длины
}

/// TOKEN_PRIVILEGES - привилегии токена
#[repr(C)]
pub struct TOKEN_PRIVILEGES {
    pub privilege_count: ULONG,
    pub privileges: [LUID_AND_ATTRIBUTES; 1], // Переменной длины
}

/// TOKEN_OWNER - владелец по умолчанию
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_OWNER {
    pub owner: *mut SID,
}

/// TOKEN_PRIMARY_GROUP - первичная группа
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_PRIMARY_GROUP {
    pub primary_group: *mut SID,
}

/// TOKEN_DEFAULT_DACL - DACL по умолчанию
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_DEFAULT_DACL {
    pub default_dacl: *mut ACL,
}

/// TOKEN_MANDATORY_LABEL - уровень целостности (MIC)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_MANDATORY_LABEL {
    pub label: SID_AND_ATTRIBUTES,
}

/// TOKEN_MANDATORY_POLICY - политика MIC
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TOKEN_MANDATORY_POLICY {
    pub policy: ULONG,
}

pub const TOKEN_MANDATORY_POLICY_OFF: ULONG = 0x0;
pub const TOKEN_MANDATORY_POLICY_NO_WRITE_UP: ULONG = 0x1;
pub const TOKEN_MANDATORY_POLICY_NEW_PROCESS_MIN: ULONG = 0x2;
pub const TOKEN_MANDATORY_POLICY_VALID_MASK: ULONG = 0x3;

/// TOKEN_ELEVATION - состояние elevation
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TOKEN_ELEVATION {
    pub token_is_elevated: ULONG,
}

/// TOKEN_LINKED_TOKEN - связанный токен (UAC)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TOKEN_LINKED_TOKEN {
    pub linked_token: PVOID, // HANDLE
}

/// TOKEN_ORIGIN - источник логона
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TOKEN_ORIGIN {
    pub originating_logon_session: LUID,
}

// =============================================================================
// Well-known privilege LUIDs
// =============================================================================

// Привилегии определяются как (high_part=0, low_part=index)
// Индексы соответствуют Win7

pub const SE_MIN_WELL_KNOWN_PRIVILEGE: ULONG = 2;
pub const SE_CREATE_TOKEN_PRIVILEGE: ULONG = 2;
pub const SE_ASSIGNPRIMARYTOKEN_PRIVILEGE: ULONG = 3;
pub const SE_LOCK_MEMORY_PRIVILEGE: ULONG = 4;
pub const SE_INCREASE_QUOTA_PRIVILEGE: ULONG = 5;
pub const SE_MACHINE_ACCOUNT_PRIVILEGE: ULONG = 6;
pub const SE_TCB_PRIVILEGE: ULONG = 7;
pub const SE_SECURITY_PRIVILEGE: ULONG = 8;
pub const SE_TAKE_OWNERSHIP_PRIVILEGE: ULONG = 9;
pub const SE_LOAD_DRIVER_PRIVILEGE: ULONG = 10;
pub const SE_SYSTEM_PROFILE_PRIVILEGE: ULONG = 11;
pub const SE_SYSTEMTIME_PRIVILEGE: ULONG = 12;
pub const SE_PROF_SINGLE_PROCESS_PRIVILEGE: ULONG = 13;
pub const SE_INC_BASE_PRIORITY_PRIVILEGE: ULONG = 14;
pub const SE_CREATE_PAGEFILE_PRIVILEGE: ULONG = 15;
pub const SE_CREATE_PERMANENT_PRIVILEGE: ULONG = 16;
pub const SE_BACKUP_PRIVILEGE: ULONG = 17;
pub const SE_RESTORE_PRIVILEGE: ULONG = 18;
pub const SE_SHUTDOWN_PRIVILEGE: ULONG = 19;
pub const SE_DEBUG_PRIVILEGE: ULONG = 20;
pub const SE_AUDIT_PRIVILEGE: ULONG = 21;
pub const SE_SYSTEM_ENVIRONMENT_PRIVILEGE: ULONG = 22;
pub const SE_CHANGE_NOTIFY_PRIVILEGE: ULONG = 23;
pub const SE_REMOTE_SHUTDOWN_PRIVILEGE: ULONG = 24;
pub const SE_UNDOCK_PRIVILEGE: ULONG = 25;
pub const SE_SYNC_AGENT_PRIVILEGE: ULONG = 26;
pub const SE_ENABLE_DELEGATION_PRIVILEGE: ULONG = 27;
pub const SE_MANAGE_VOLUME_PRIVILEGE: ULONG = 28;
pub const SE_IMPERSONATE_PRIVILEGE: ULONG = 29;
pub const SE_CREATE_GLOBAL_PRIVILEGE: ULONG = 30;
pub const SE_TRUSTED_CREDMAN_ACCESS_PRIVILEGE: ULONG = 31;
pub const SE_RELABEL_PRIVILEGE: ULONG = 32;
pub const SE_INC_WORKING_SET_PRIVILEGE: ULONG = 33;
pub const SE_TIME_ZONE_PRIVILEGE: ULONG = 34;
pub const SE_CREATE_SYMBOLIC_LINK_PRIVILEGE: ULONG = 35;
pub const SE_MAX_WELL_KNOWN_PRIVILEGE: ULONG = SE_CREATE_SYMBOLIC_LINK_PRIVILEGE;

/// Количество известных привилегий
pub const SE_PRIVILEGE_COUNT: usize = (SE_MAX_WELL_KNOWN_PRIVILEGE - SE_MIN_WELL_KNOWN_PRIVILEGE + 1) as usize;

// =============================================================================
// GENERIC_MAPPING - реэкспорт из ob::types для совместимости
// =============================================================================

// Примечание: GENERIC_MAPPING определён в ob::types, здесь только type alias
// для удобства использования в SE модулях.
// При необходимости используйте crate::ob::types::GENERIC_MAPPING напрямую.

