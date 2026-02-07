//! Token - внутренняя структура токена
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/token.c

use core::sync::atomic::{AtomicU32, AtomicPtr, Ordering};

use crate::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use crate::nt::{
    NTSTATUS, PVOID, STATUS_SUCCESS, STATUS_INSUFFICIENT_RESOURCES, ULONG, LARGE_INTEGER,
    LUID, LUID_AND_ATTRIBUTES, SID, SID_AND_ATTRIBUTES, ACL,
    TOKEN_TYPE, TOKEN_SOURCE, TOKEN_STATISTICS,
    SECURITY_IMPERSONATION_LEVEL, TOKEN_ELEVATION_TYPE,
    SE_GROUP_ENABLED, SE_GROUP_ENABLED_BY_DEFAULT, SE_GROUP_MANDATORY,
    SE_PRIVILEGE_ENABLED, SE_PRIVILEGE_ENABLED_BY_DEFAULT,
    SECURITY_MANDATORY_SYSTEM_RID,
};

use crate::se::sid::{
    se_local_system_sid, se_world_sid, se_authenticated_users_sid,
    se_aliased_admins_sid, se_system_mandatory_sid, rtl_length_sid,
};
use crate::se::priv_::SE_SYSTEM_TOKEN_PRIVILEGES;

/// Pool tag для Token аллокаций
const TAG_TOKEN: u32 = u32::from_le_bytes(*b"koTe");

// =============================================================================
// TOKEN structure
// =============================================================================

/// TOKEN - внутренняя структура токена
///
/// Хранит security context: user, groups, privileges, default DACL и т.д.
///
/// # Layout
/// Соответствует ReactOS TOKEN structure.
#[repr(C)]
pub struct TOKEN {
    /// Token source (идентификатор создателя токена)
    pub token_source: TOKEN_SOURCE,
    /// Token ID (уникальный идентификатор токена)
    pub token_id: LUID,
    /// Authentication ID (logon session)
    pub authentication_id: LUID,
    /// Parent token ID (для impersonation)
    pub parent_token_id: LUID,
    /// Expiration time
    pub expiration_time: LARGE_INTEGER,
    /// Modified ID (изменяется при каждой модификации токена)
    pub modified_id: LUID,

    /// Token type (Primary / Impersonation)
    pub token_type: TOKEN_TYPE,
    /// Impersonation level (для impersonation tokens)
    pub impersonation_level: SECURITY_IMPERSONATION_LEVEL,

    /// Flags
    pub token_flags: AtomicU32,

    /// Session ID
    pub session_id: u32,

    // --- User ---
    /// User SID (указатель внутрь variable_part)
    pub user_sid: *mut SID,

    // --- Groups ---
    /// Количество групп
    pub group_count: u32,
    /// Массив групп (указатель внутрь variable_part)
    pub groups: *mut SID_AND_ATTRIBUTES,
    /// Restricted SIDs count
    pub restricted_sid_count: u32,
    /// Restricted SIDs
    pub restricted_sids: *mut SID_AND_ATTRIBUTES,

    // --- Privileges ---
    /// Количество привилегий
    pub privilege_count: u32,
    /// Массив привилегий (указатель внутрь variable_part)
    pub privileges: *mut LUID_AND_ATTRIBUTES,

    // --- Default Security ---
    /// Owner SID for new objects
    pub owner: *mut SID,
    /// Primary group SID
    pub primary_group: *mut SID,
    /// Default DACL for new objects
    pub default_dacl: *mut ACL,

    // --- UAC / Integrity (Win7) ---
    /// Elevation type
    pub elevation_type: TOKEN_ELEVATION_TYPE,
    /// Elevation status (is elevated)
    pub elevation: u32,
    /// Linked token (для UAC)
    pub linked_token: AtomicPtr<TOKEN>,

    // --- Integrity Level ---
    /// Integrity level SID
    pub integrity_level_sid: *mut SID,
    /// Integrity level index (cached RID value)
    pub integrity_level_index: u32,

    // --- Statistics ---
    /// Dynamic charged (bytes)
    pub dynamic_charged: u32,
    /// Dynamic available (bytes remaining)
    pub dynamic_available: u32,

    // --- Variable Part ---
    /// Начало variable part (SIDs, privileges хранятся здесь)
    /// Размер variable_part зависит от количества групп/привилегий
    pub variable_part: [u8; 0],
}

// Token flags
pub const TOKEN_HAS_TRAVERSE_PRIVILEGE: u32 = 0x0001;
pub const TOKEN_HAS_BACKUP_PRIVILEGE: u32 = 0x0002;
pub const TOKEN_HAS_RESTORE_PRIVILEGE: u32 = 0x0004;
pub const TOKEN_HAS_ADMIN_GROUP: u32 = 0x0008;
pub const TOKEN_IS_RESTRICTED: u32 = 0x0010;
pub const TOKEN_SESSION_NOT_REFERENCED: u32 = 0x0020;
pub const TOKEN_SANDBOX_INERT: u32 = 0x0040;
pub const TOKEN_HAS_IMPERSONATE_PRIVILEGE: u32 = 0x0080;
pub const TOKEN_VIRTUALIZE_ALLOWED: u32 = 0x0200;
pub const TOKEN_VIRTUALIZE_ENABLED: u32 = 0x0400;
pub const TOKEN_IS_FILTERED: u32 = 0x0800;
pub const TOKEN_UIACCESS: u32 = 0x1000;
pub const TOKEN_NOT_LOW: u32 = 0x2000;
pub const TOKEN_LOWBOX: u32 = 0x4000;
pub const TOKEN_HAS_OWN_CLAIM_ATTRIBUTES: u32 = 0x8000;
pub const TOKEN_PRIVATE_NAMESPACE: u32 = 0x10000;
pub const TOKEN_DO_NOT_USE_GLOBAL_ATTRIBS_FOR_QUERY: u32 = 0x20000;

impl TOKEN {
    /// Возвращает TOKEN_STATISTICS для токена
    pub fn get_statistics(&self) -> TOKEN_STATISTICS {
        TOKEN_STATISTICS {
            token_id: self.token_id,
            authentication_id: self.authentication_id,
            expiration_time: self.expiration_time,
            token_type: self.token_type,
            impersonation_level: self.impersonation_level,
            dynamic_charged: self.dynamic_charged,
            dynamic_available: self.dynamic_available,
            group_count: self.group_count,
            privilege_count: self.privilege_count,
            modified_id: self.modified_id,
        }
    }

    /// Проверяет, является ли токен restricted
    #[inline]
    pub fn is_restricted(&self) -> bool {
        (self.token_flags.load(Ordering::Acquire) & TOKEN_IS_RESTRICTED) != 0
            || self.restricted_sid_count > 0
    }

    /// Проверяет, является ли токен elevated (UAC)
    #[inline]
    pub fn is_elevated(&self) -> bool {
        self.elevation != 0
    }

    /// Возвращает integrity level (RID)
    #[inline]
    pub fn integrity_level(&self) -> u32 {
        self.integrity_level_index
    }
}

// =============================================================================
// Token creation
// =============================================================================

/// Глобальный счетчик для генерации уникальных Token ID
static NEXT_TOKEN_ID: AtomicU32 = AtomicU32::new(1);

/// Генерирует новый уникальный Token ID
fn generate_token_id() -> LUID {
    let id = NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst);
    LUID::new(id, 0)
}

/// SepCreateToken - создает новый токен
///
/// Внутренняя функция SE для создания токенов.
///
/// # Параметры
/// - `token_type` - тип токена (Primary/Impersonation)
/// - `impersonation_level` - уровень impersonation
/// - `authentication_id` - logon session ID
/// - `user` - user SID
/// - `groups` - массив групп
/// - `group_count` - количество групп
/// - `privileges` - массив привилегий
/// - `privilege_count` - количество привилегий
/// - `owner` - owner SID (может быть null)
/// - `primary_group` - primary group SID
/// - `default_dacl` - default DACL (может быть null)
/// - `source` - источник токена
///
/// # Возвращает
/// Указатель на созданный TOKEN или null при ошибке
#[allow(clippy::too_many_arguments)]
pub fn sep_create_token(
    token_type: TOKEN_TYPE,
    impersonation_level: SECURITY_IMPERSONATION_LEVEL,
    authentication_id: LUID,
    user: *const SID,
    groups: *const SID_AND_ATTRIBUTES,
    group_count: u32,
    privileges: *const LUID_AND_ATTRIBUTES,
    privilege_count: u32,
    owner: *const SID,
    primary_group: *const SID,
    default_dacl: *const ACL,
    source: &TOKEN_SOURCE,
    integrity_sid: *const SID,
) -> *mut TOKEN {
    if user.is_null() || primary_group.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        // Вычисляем размер variable part
        let user_len = rtl_length_sid(user) as usize;
        let primary_group_len = rtl_length_sid(primary_group) as usize;
        
        let mut groups_size = 0usize;
        let mut groups_sids_size = 0usize;
        if !groups.is_null() && group_count > 0 {
            groups_size = group_count as usize * core::mem::size_of::<SID_AND_ATTRIBUTES>();
            for i in 0..group_count as usize {
                let g = &*groups.add(i);
                if !g.sid.is_null() {
                    groups_sids_size += rtl_length_sid(g.sid) as usize;
                }
            }
        }

        let privileges_size = privilege_count as usize * core::mem::size_of::<LUID_AND_ATTRIBUTES>();

        let owner_len = if !owner.is_null() { rtl_length_sid(owner) as usize } else { 0 };
        
        let default_dacl_len = if !default_dacl.is_null() {
            (*default_dacl).acl_size as usize
        } else {
            0
        };

        let integrity_len = if !integrity_sid.is_null() {
            rtl_length_sid(integrity_sid) as usize
        } else {
            0
        };

        let variable_size = user_len + primary_group_len + groups_size + groups_sids_size
            + privileges_size + owner_len + default_dacl_len + integrity_len;

        let total_size = core::mem::size_of::<TOKEN>() + variable_size;

        // Аллоцируем
        let token_ptr = ex_allocate_pool_with_tag(POOL_TYPE::NonPagedPool, total_size, TAG_TOKEN);
        if token_ptr.is_null() {
            return core::ptr::null_mut();
        }

        // Обнуляем
        core::ptr::write_bytes(token_ptr, 0, total_size);

        let token = token_ptr as *mut TOKEN;
        let mut var_ptr = (token as *mut u8).add(core::mem::size_of::<TOKEN>());

        // Инициализируем фиксированные поля
        (*token).token_source = *source;
        (*token).token_id = generate_token_id();
        (*token).authentication_id = authentication_id;
        (*token).parent_token_id = LUID::new(0, 0);
        (*token).expiration_time = LARGE_INTEGER::new(i64::MAX); // Never expires
        (*token).modified_id = (*token).token_id;
        (*token).token_type = token_type;
        (*token).impersonation_level = impersonation_level;
        (*token).token_flags = AtomicU32::new(0);
        (*token).session_id = 0;
        (*token).dynamic_charged = variable_size as u32;
        (*token).dynamic_available = 0;

        // UAC defaults
        (*token).elevation_type = TOKEN_ELEVATION_TYPE::TokenElevationTypeDefault;
        (*token).elevation = 0;
        (*token).linked_token = AtomicPtr::new(core::ptr::null_mut());

        // Копируем User SID
        (*token).user_sid = var_ptr as *mut SID;
        core::ptr::copy_nonoverlapping(user as *const u8, var_ptr, user_len);
        var_ptr = var_ptr.add(user_len);

        // Копируем Primary Group SID
        (*token).primary_group = var_ptr as *mut SID;
        core::ptr::copy_nonoverlapping(primary_group as *const u8, var_ptr, primary_group_len);
        var_ptr = var_ptr.add(primary_group_len);

        // Копируем Groups
        (*token).group_count = group_count;
        if group_count > 0 && !groups.is_null() {
            (*token).groups = var_ptr as *mut SID_AND_ATTRIBUTES;
            let groups_array = var_ptr as *mut SID_AND_ATTRIBUTES;
            var_ptr = var_ptr.add(groups_size);

            for i in 0..group_count as usize {
                let src_group = &*groups.add(i);
                let dst_group = &mut *groups_array.add(i);

                dst_group.attributes = src_group.attributes;
                if !src_group.sid.is_null() {
                    let sid_len = rtl_length_sid(src_group.sid) as usize;
                    dst_group.sid = var_ptr as *mut SID;
                    core::ptr::copy_nonoverlapping(src_group.sid as *const u8, var_ptr, sid_len);
                    var_ptr = var_ptr.add(sid_len);
                } else {
                    dst_group.sid = core::ptr::null_mut();
                }
            }
        } else {
            (*token).groups = core::ptr::null_mut();
        }

        // Restricted SIDs (пока не поддерживаем при создании)
        (*token).restricted_sid_count = 0;
        (*token).restricted_sids = core::ptr::null_mut();

        // Копируем Privileges
        (*token).privilege_count = privilege_count;
        if privilege_count > 0 && !privileges.is_null() {
            (*token).privileges = var_ptr as *mut LUID_AND_ATTRIBUTES;
            core::ptr::copy_nonoverlapping(
                privileges as *const u8,
                var_ptr,
                privileges_size,
            );
            var_ptr = var_ptr.add(privileges_size);

            // Устанавливаем флаги токена на основе привилегий
            sep_update_token_flags_from_privileges(token);
        } else {
            (*token).privileges = core::ptr::null_mut();
        }

        // Owner (может быть null - тогда используется user)
        if !owner.is_null() {
            (*token).owner = var_ptr as *mut SID;
            core::ptr::copy_nonoverlapping(owner as *const u8, var_ptr, owner_len);
            var_ptr = var_ptr.add(owner_len);
        } else {
            (*token).owner = (*token).user_sid; // Default: user is owner
        }

        // Default DACL
        if !default_dacl.is_null() {
            (*token).default_dacl = var_ptr as *mut ACL;
            core::ptr::copy_nonoverlapping(default_dacl as *const u8, var_ptr, default_dacl_len);
            var_ptr = var_ptr.add(default_dacl_len);
        } else {
            (*token).default_dacl = core::ptr::null_mut();
        }

        // Integrity Level
        if !integrity_sid.is_null() {
            (*token).integrity_level_sid = var_ptr as *mut SID;
            core::ptr::copy_nonoverlapping(integrity_sid as *const u8, var_ptr, integrity_len);
            (*token).integrity_level_index = crate::se::sid::rtl_get_rid(integrity_sid);
        } else {
            // Default: Medium integrity
            (*token).integrity_level_sid = core::ptr::null_mut();
            (*token).integrity_level_index = crate::nt::SECURITY_MANDATORY_MEDIUM_RID;
        }

        // Проверяем наличие Administrators group
        sep_update_token_flags_from_groups(token);

        token
    }
}

/// Обновляет флаги токена на основе привилегий
unsafe fn sep_update_token_flags_from_privileges(token: *mut TOKEN) {
    use crate::se::priv_::{SE_CHANGE_NOTIFY_LUID, SE_BACKUP_LUID, SE_RESTORE_LUID, SE_IMPERSONATE_LUID};

    let mut flags = (*token).token_flags.load(Ordering::Acquire);

    for i in 0..(*token).privilege_count as usize {
        let priv_ = &*(*token).privileges.add(i);
        
        if (priv_.attributes & SE_PRIVILEGE_ENABLED) != 0 {
            if priv_.luid.low_part == SE_CHANGE_NOTIFY_LUID.low_part {
                flags |= TOKEN_HAS_TRAVERSE_PRIVILEGE;
            }
            if priv_.luid.low_part == SE_BACKUP_LUID.low_part {
                flags |= TOKEN_HAS_BACKUP_PRIVILEGE;
            }
            if priv_.luid.low_part == SE_RESTORE_LUID.low_part {
                flags |= TOKEN_HAS_RESTORE_PRIVILEGE;
            }
            if priv_.luid.low_part == SE_IMPERSONATE_LUID.low_part {
                flags |= TOKEN_HAS_IMPERSONATE_PRIVILEGE;
            }
        }
    }

    (*token).token_flags.store(flags, Ordering::Release);
}

/// Обновляет флаги токена на основе групп
unsafe fn sep_update_token_flags_from_groups(token: *mut TOKEN) {
    use crate::se::sid::rtl_equal_sid;

    let mut flags = (*token).token_flags.load(Ordering::Acquire);

    let admins_sid = se_aliased_admins_sid();

    for i in 0..(*token).group_count as usize {
        let group = &*(*token).groups.add(i);
        
        if (group.attributes & SE_GROUP_ENABLED) != 0 {
            if rtl_equal_sid(group.sid, admins_sid) {
                flags |= TOKEN_HAS_ADMIN_GROUP;
                break;
            }
        }
    }

    (*token).token_flags.store(flags, Ordering::Release);
}

/// Освобождает токен
pub fn sep_delete_token(token: *mut TOKEN) {
    if !token.is_null() {
        ex_free_pool_with_tag(token as PVOID, TAG_TOKEN);
    }
}

// =============================================================================
// System Token creation
// =============================================================================

/// Создает System token (для System process)
///
/// System token имеет:
/// - User: LocalSystem (S-1-5-18)
/// - Groups: Administrators, Authenticated Users, Everyone
/// - Privileges: все привилегии enabled
/// - Integrity: System
pub fn sep_create_system_token() -> *mut TOKEN {
    // System token source
    // Конвертируем u8 в i8 для source_name
    let name_bytes: [i8; 8] = [
        b'*' as i8, b'S' as i8, b'Y' as i8, b'S' as i8,
        b'T' as i8, b'E' as i8, b'M' as i8, b'*' as i8,
    ];
    let source = TOKEN_SOURCE {
        source_name: name_bytes,
        source_identifier: LUID::new(0, 0),
    };

    // System authentication ID (SYSTEM_LUID)
    let auth_id = LUID::new(0x3e7, 0); // SYSTEM_LUID = 0x3e7

    // User: LocalSystem
    let user = se_local_system_sid();

    // Groups
    let groups: [SID_AND_ATTRIBUTES; 3] = [
        SID_AND_ATTRIBUTES {
            sid: se_aliased_admins_sid() as *mut SID,
            attributes: SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_MANDATORY,
        },
        SID_AND_ATTRIBUTES {
            sid: se_authenticated_users_sid() as *mut SID,
            attributes: SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_MANDATORY,
        },
        SID_AND_ATTRIBUTES {
            sid: se_world_sid() as *mut SID,
            attributes: SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_MANDATORY,
        },
    ];

    sep_create_token(
        TOKEN_TYPE::TokenPrimary,
        SECURITY_IMPERSONATION_LEVEL::SecurityImpersonation,
        auth_id,
        user,
        groups.as_ptr(),
        groups.len() as u32,
        SE_SYSTEM_TOKEN_PRIVILEGES.as_ptr(),
        SE_SYSTEM_TOKEN_PRIVILEGES.len() as u32,
        core::ptr::null(), // owner = user
        user,              // primary group = user
        core::ptr::null(), // no default DACL
        &source,
        se_system_mandatory_sid(), // System integrity
    )
}

