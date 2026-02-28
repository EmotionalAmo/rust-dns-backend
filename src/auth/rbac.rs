#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub enum Permission {
    ReadDashboard,
    ReadQueryLog,
    ManageFilters,
    ManageClients,
    ManageSettings,
    ReadAuditLog,
    ManageUsers,
}

pub fn has_permission(role: &str, permission: &Permission) -> bool {
    match role {
        "super_admin" => true,
        "admin" => !matches!(permission, Permission::ManageUsers),
        "operator" => matches!(
            permission,
            Permission::ReadDashboard
                | Permission::ReadQueryLog
                | Permission::ManageFilters
                | Permission::ManageClients
        ),
        "read_only" => matches!(
            permission,
            Permission::ReadDashboard | Permission::ReadQueryLog
        ),
        _ => false,
    }
}
