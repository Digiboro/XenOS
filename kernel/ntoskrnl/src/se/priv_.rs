//! Privileges - привилегии NT
//!
//! Управление привилегиями: таблицы, проверка, включение/отключение.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/priv.c

use core::sync::atomic::{AtomicU64, Ordering};

use crate::nt::{
    BOOLEAN, NTSTATUS, STATUS_SUCCESS, STATUS_PRIVILEGE_NOT_HELD, STATUS_NOT_ALL_ASSIGNED,
    ULONG, LUID, LUID_AND_ATTRIBUTES, PRIVILEGE_SET,
    SE_PRIVILEGE_ENABLED, SE_PRIVILEGE_ENABLED_BY_DEFAULT, SE_PRIVILEGE_USED_FOR_ACCESS,
    SE_MIN_WELL_KNOWN_PRIVILEGE, SE_MAX_WELL_KNOWN_PRIVILEGE, SE_PRIVILEGE_COUNT,
    SE_TCB_PRIVILEGE, SE_DEBUG_PRIVILEGE, SE_SECURITY_PRIVILEGE, SE_TAKE_OWNERSHIP_PRIVILEGE,
    SE_BACKUP_PRIVILEGE, SE_RESTORE_PRIVILEGE, SE_IMPERSONATE_PRIVILEGE,
    SE_ASSIGNPRIMARYTOKEN_PRIVILEGE, SE_INCREASE_QUOTA_PRIVILEGE, SE_LOAD_DRIVER_PRIVILEGE,
    SE_AUDIT_PRIVILEGE, SE_CHANGE_NOTIFY_PRIVILEGE, SE_CREATE_GLOBAL_PRIVILEGE,
};

// =============================================================================
// Privilege LUID constants
// =============================================================================

/// SeCreateTokenPrivilege
pub const SE_CREATE_TOKEN_LUID: LUID = LUID::new(2, 0);
/// SeAssignPrimaryTokenPrivilege
pub const SE_ASSIGNPRIMARYTOKEN_LUID: LUID = LUID::new(3, 0);
/// SeLockMemoryPrivilege
pub const SE_LOCK_MEMORY_LUID: LUID = LUID::new(4, 0);
/// SeIncreaseQuotaPrivilege
pub const SE_INCREASE_QUOTA_LUID: LUID = LUID::new(5, 0);
/// SeMachineAccountPrivilege
pub const SE_MACHINE_ACCOUNT_LUID: LUID = LUID::new(6, 0);
/// SeTcbPrivilege
pub const SE_TCB_LUID: LUID = LUID::new(7, 0);
/// SeSecurityPrivilege
pub const SE_SECURITY_LUID: LUID = LUID::new(8, 0);
/// SeTakeOwnershipPrivilege
pub const SE_TAKE_OWNERSHIP_LUID: LUID = LUID::new(9, 0);
/// SeLoadDriverPrivilege
pub const SE_LOAD_DRIVER_LUID: LUID = LUID::new(10, 0);
/// SeSystemProfilePrivilege
pub const SE_SYSTEM_PROFILE_LUID: LUID = LUID::new(11, 0);
/// SeSystemtimePrivilege
pub const SE_SYSTEMTIME_LUID: LUID = LUID::new(12, 0);
/// SeProfileSingleProcessPrivilege
pub const SE_PROF_SINGLE_PROCESS_LUID: LUID = LUID::new(13, 0);
/// SeIncreaseBasePriorityPrivilege
pub const SE_INC_BASE_PRIORITY_LUID: LUID = LUID::new(14, 0);
/// SeCreatePagefilePrivilege
pub const SE_CREATE_PAGEFILE_LUID: LUID = LUID::new(15, 0);
/// SeCreatePermanentPrivilege
pub const SE_CREATE_PERMANENT_LUID: LUID = LUID::new(16, 0);
/// SeBackupPrivilege
pub const SE_BACKUP_LUID: LUID = LUID::new(17, 0);
/// SeRestorePrivilege
pub const SE_RESTORE_LUID: LUID = LUID::new(18, 0);
/// SeShutdownPrivilege
pub const SE_SHUTDOWN_LUID: LUID = LUID::new(19, 0);
/// SeDebugPrivilege
pub const SE_DEBUG_LUID: LUID = LUID::new(20, 0);
/// SeAuditPrivilege
pub const SE_AUDIT_LUID: LUID = LUID::new(21, 0);
/// SeSystemEnvironmentPrivilege
pub const SE_SYSTEM_ENVIRONMENT_LUID: LUID = LUID::new(22, 0);
/// SeChangeNotifyPrivilege
pub const SE_CHANGE_NOTIFY_LUID: LUID = LUID::new(23, 0);
/// SeRemoteShutdownPrivilege
pub const SE_REMOTE_SHUTDOWN_LUID: LUID = LUID::new(24, 0);
/// SeUndockPrivilege
pub const SE_UNDOCK_LUID: LUID = LUID::new(25, 0);
/// SeSyncAgentPrivilege
pub const SE_SYNC_AGENT_LUID: LUID = LUID::new(26, 0);
/// SeEnableDelegationPrivilege
pub const SE_ENABLE_DELEGATION_LUID: LUID = LUID::new(27, 0);
/// SeManageVolumePrivilege
pub const SE_MANAGE_VOLUME_LUID: LUID = LUID::new(28, 0);
/// SeImpersonatePrivilege
pub const SE_IMPERSONATE_LUID: LUID = LUID::new(29, 0);
/// SeCreateGlobalPrivilege
pub const SE_CREATE_GLOBAL_LUID: LUID = LUID::new(30, 0);
/// SeTrustedCredManAccessPrivilege
pub const SE_TRUSTED_CREDMAN_ACCESS_LUID: LUID = LUID::new(31, 0);
/// SeRelabelPrivilege
pub const SE_RELABEL_LUID: LUID = LUID::new(32, 0);
/// SeIncreaseWorkingSetPrivilege
pub const SE_INC_WORKING_SET_LUID: LUID = LUID::new(33, 0);
/// SeTimeZonePrivilege
pub const SE_TIME_ZONE_LUID: LUID = LUID::new(34, 0);
/// SeCreateSymbolicLinkPrivilege
pub const SE_CREATE_SYMBOLIC_LINK_LUID: LUID = LUID::new(35, 0);

// =============================================================================
// Privilege names (for future use with LookupPrivilegeValue/Name)
// =============================================================================

/// Таблица имен привилегий (индекс = LUID.low_part - SE_MIN_WELL_KNOWN_PRIVILEGE)
pub static SE_PRIVILEGE_NAMES: [&str; SE_PRIVILEGE_COUNT] = [
    "SeCreateTokenPrivilege",           // 2
    "SeAssignPrimaryTokenPrivilege",    // 3
    "SeLockMemoryPrivilege",            // 4
    "SeIncreaseQuotaPrivilege",         // 5
    "SeMachineAccountPrivilege",        // 6
    "SeTcbPrivilege",                   // 7
    "SeSecurityPrivilege",              // 8
    "SeTakeOwnershipPrivilege",         // 9
    "SeLoadDriverPrivilege",            // 10
    "SeSystemProfilePrivilege",         // 11
    "SeSystemtimePrivilege",            // 12
    "SeProfileSingleProcessPrivilege",  // 13
    "SeIncreaseBasePriorityPrivilege",  // 14
    "SeCreatePagefilePrivilege",        // 15
    "SeCreatePermanentPrivilege",       // 16
    "SeBackupPrivilege",                // 17
    "SeRestorePrivilege",               // 18
    "SeShutdownPrivilege",              // 19
    "SeDebugPrivilege",                 // 20
    "SeAuditPrivilege",                 // 21
    "SeSystemEnvironmentPrivilege",     // 22
    "SeChangeNotifyPrivilege",          // 23
    "SeRemoteShutdownPrivilege",        // 24
    "SeUndockPrivilege",                // 25
    "SeSyncAgentPrivilege",             // 26
    "SeEnableDelegationPrivilege",      // 27
    "SeManageVolumePrivilege",          // 28
    "SeImpersonatePrivilege",           // 29
    "SeCreateGlobalPrivilege",          // 30
    "SeTrustedCredManAccessPrivilege",  // 31
    "SeRelabelPrivilege",               // 32
    "SeIncreaseWorkingSetPrivilege",    // 33
    "SeTimeZonePrivilege",              // 34
    "SeCreateSymbolicLinkPrivilege",    // 35
];

// =============================================================================
// Privilege check functions
// =============================================================================

/// Проверяет, является ли LUID well-known привилегией
#[inline]
pub fn se_is_well_known_privilege(luid: &LUID) -> bool {
    luid.high_part == 0 
        && luid.low_part >= SE_MIN_WELL_KNOWN_PRIVILEGE 
        && luid.low_part <= SE_MAX_WELL_KNOWN_PRIVILEGE
}

/// Получает индекс привилегии в массиве (0-based)
#[inline]
pub fn se_privilege_index(luid: &LUID) -> Option<usize> {
    if se_is_well_known_privilege(luid) {
        Some((luid.low_part - SE_MIN_WELL_KNOWN_PRIVILEGE) as usize)
    } else {
        None
    }
}

/// SepPrivilegeCheck - внутренняя проверка привилегий
///
/// Проверяет, есть ли у токена все запрошенные привилегии.
///
/// # Параметры
/// - `privileges` - массив привилегий токена
/// - `privilege_count` - количество привилегий в токене
/// - `required` - набор требуемых привилегий
/// - `privilege_set_control` - PRIVILEGE_SET_ALL_NECESSARY или 0
///
/// # Возвращает
/// true если проверка прошла успешно
pub fn sep_privilege_check(
    privileges: *const LUID_AND_ATTRIBUTES,
    privilege_count: ULONG,
    required: *mut PRIVILEGE_SET,
    privilege_set_control: ULONG,
) -> bool {
    use crate::nt::PRIVILEGE_SET_ALL_NECESSARY;

    if required.is_null() {
        return true;
    }

    unsafe {
        let required_count = (*required).privilege_count;
        if required_count == 0 {
            return true;
        }

        let all_necessary = ((*required).control & PRIVILEGE_SET_ALL_NECESSARY) != 0
            || privilege_set_control != 0;

        let mut found_count = 0u32;

        // Для каждой требуемой привилегии
        for i in 0..required_count as usize {
            let req_priv = &mut *(*required).privilege.as_mut_ptr().add(i);
            let req_luid = &req_priv.luid;

            // Ищем в привилегиях токена
            let mut found = false;
            for j in 0..privilege_count as usize {
                let token_priv = &*privileges.add(j);

                // Сравниваем LUID
                if token_priv.luid.low_part == req_luid.low_part
                    && token_priv.luid.high_part == req_luid.high_part
                {
                    // Проверяем, что привилегия enabled
                    if (token_priv.attributes & SE_PRIVILEGE_ENABLED) != 0 {
                        found = true;
                        req_priv.attributes |= SE_PRIVILEGE_USED_FOR_ACCESS;
                        found_count += 1;
                        break;
                    }
                }
            }

            // Если требуются все привилегии и одна не найдена - сразу fail
            if all_necessary && !found {
                return false;
            }
        }

        // Если не требуются все - достаточно найти хотя бы одну
        if all_necessary {
            found_count == required_count
        } else {
            found_count > 0
        }
    }
}

/// SePrivilegeCheck - проверка привилегий
///
/// Exported API для проверки привилегий.
///
/// # Источник
/// ReactOS: ntoskrnl/se/priv.c SePrivilegeCheck
pub fn se_privilege_check(
    privileges: *const LUID_AND_ATTRIBUTES,
    privilege_count: ULONG,
    required: *mut PRIVILEGE_SET,
    previous_mode: i32, // KPROCESSOR_MODE
) -> bool {
    // Для KernelMode (0) не требуется проверка привилегий
    if previous_mode == 0 {
        return true;
    }

    sep_privilege_check(privileges, privilege_count, required, 0)
}

/// SeSinglePrivilegeCheck - проверка одной привилегии
///
/// Проверяет, есть ли у текущего потока указанная привилегия.
///
/// # Источник
/// ReactOS: ntoskrnl/se/priv.c SeSinglePrivilegeCheck
pub fn se_single_privilege_check(
    privilege_luid: LUID,
    previous_mode: i32, // KPROCESSOR_MODE
) -> bool {
    // Для KernelMode (0) всегда успех
    if previous_mode == 0 {
        return true;
    }

    // TODO: получить токен текущего потока и проверить привилегию
    // Пока заглушка - возвращаем false для UserMode
    false
}

/// SepTokenHasPrivilege - проверяет наличие привилегии в токене
///
/// Внутренняя функция SE.
pub fn sep_token_has_privilege(
    privileges: *const LUID_AND_ATTRIBUTES,
    privilege_count: ULONG,
    privilege_luid: &LUID,
) -> bool {
    if privileges.is_null() || privilege_count == 0 {
        return false;
    }

    unsafe {
        for i in 0..privilege_count as usize {
            let priv_ = &*privileges.add(i);

            if priv_.luid.low_part == privilege_luid.low_part
                && priv_.luid.high_part == privilege_luid.high_part
            {
                // Привилегия должна быть enabled
                return (priv_.attributes & SE_PRIVILEGE_ENABLED) != 0;
            }
        }
    }

    false
}

/// SepTokenHasAnyPrivilege - проверяет наличие хотя бы одной из привилегий
pub fn sep_token_has_any_privilege(
    privileges: *const LUID_AND_ATTRIBUTES,
    privilege_count: ULONG,
    check_luids: &[LUID],
) -> bool {
    for luid in check_luids {
        if sep_token_has_privilege(privileges, privilege_count, luid) {
            return true;
        }
    }
    false
}

// =============================================================================
// System token privileges (для создания System token)
// =============================================================================

/// Привилегии для System token (LocalSystem account)
///
/// System процесс получает все привилегии enabled by default.
pub const SE_SYSTEM_TOKEN_PRIVILEGES: [LUID_AND_ATTRIBUTES; SE_PRIVILEGE_COUNT] = [
    LUID_AND_ATTRIBUTES::new(LUID::new(2, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeCreateTokenPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(3, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeAssignPrimaryTokenPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(4, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeLockMemoryPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(5, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeIncreaseQuotaPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(6, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeMachineAccountPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(7, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeTcbPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(8, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeSecurityPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(9, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT),  // SeTakeOwnershipPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(10, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeLoadDriverPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(11, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeSystemProfilePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(12, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeSystemtimePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(13, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeProfileSingleProcessPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(14, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeIncreaseBasePriorityPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(15, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeCreatePagefilePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(16, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeCreatePermanentPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(17, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeBackupPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(18, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeRestorePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(19, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeShutdownPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(20, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeDebugPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(21, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeAuditPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(22, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeSystemEnvironmentPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(23, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeChangeNotifyPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(24, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeRemoteShutdownPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(25, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeUndockPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(26, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeSyncAgentPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(27, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeEnableDelegationPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(28, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeManageVolumePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(29, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeImpersonatePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(30, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeCreateGlobalPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(31, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeTrustedCredManAccessPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(32, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeRelabelPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(33, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeIncreaseWorkingSetPrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(34, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeTimeZonePrivilege
    LUID_AND_ATTRIBUTES::new(LUID::new(35, 0), SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_ENABLED_BY_DEFAULT), // SeCreateSymbolicLinkPrivilege
];

