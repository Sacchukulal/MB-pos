//! Owner authority survives old grants and role sync without widening other roles.
#![allow(clippy::expect_used, reason = "tests: expect is the assertion")]

mod common;

use common::{OUTLET, Scratch};
use mb_auth::{Permission, PermissionSet, RolePreset};
use mb_core::{Money, Timestamp};
use mb_db::repo::people::CloudRole;
use mb_db::Repos;

fn cloud_role(id: &str) -> CloudRole {
    CloudRole {
        id: id.to_owned(),
        // Display names do not confer Owner authority.
        name: "Owner".to_owned(),
        is_builtin: false,
        max_discount_bp: Some(0),
        max_discount_paise: Some(0),
        permissions: vec![Permission::BillCreate.code().to_owned()],
        updated_at: Timestamp::from_millis(2),
    }
}

#[test]
fn owner_permissions_resolve_stale_persisted_grants_without_a_migration() {
    let scratch = Scratch::new("owner_permissions_read");
    let db = scratch.open();
    db.transaction(|tx| {
        // Reproduce an existing install whose earlier cloud pull removed Owner grants.
        tx.execute_batch(
            "INSERT INTO roles (id, outlet_id, name, is_builtin, max_discount_bp,
                                max_discount_paise, updated_at)
             VALUES ('role_owner', 'outlet_default', 'Shop owner', 0, 0, 0, 1);
             INSERT INTO staff (id, outlet_id, role_id, name, status, created_at, updated_at)
             VALUES ('staff_owner', 'outlet_default', 'role_owner', 'Existing owner', 'active', 1, 1);",
        )?;
        let repos = Repos::new(tx);
        let role = repos.people().list_roles(OUTLET)?.remove(0);
        assert_eq!(role.permissions, PermissionSet::everything());
        assert_eq!(role.max_discount_bp, None);
        assert_eq!(role.max_discount, None);
        assert!(role.is_builtin);
        let owner = repos.people().find_staff(OUTLET, "staff_owner")?.expect("owner");
        assert_eq!(owner.permissions, PermissionSet::everything());
        assert_eq!(owner.max_discount_bp, None);
        assert_eq!(owner.max_discount, None);
        assert_eq!(repos.people().active_administrators(OUTLET)?, vec![owner.id]);
        Ok(())
    }).expect("existing Owner has all authority");
}

#[test]
fn owner_permissions_sync_and_local_save_cannot_restrict_owner() {
    let scratch = Scratch::new("owner_permissions_sync");
    let db = scratch.open();
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut limited = RolePreset::Owner.shape();
        limited.permissions = PermissionSet::new();
        limited.max_discount_bp = Some(0);
        limited.max_discount = Some(Money::ZERO);
        repos.people().save_role(OUTLET, &limited, Timestamp::from_millis(1))?;
        assert!(repos.people().apply_role_from_cloud(OUTLET, &cloud_role(RolePreset::Owner.id()))?);
        let stored: (i64, Option<i64>, Option<i64>) = tx.query_row(
            "SELECT is_builtin, max_discount_bp, max_discount_paise FROM roles WHERE id = 'role_owner'",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(stored, (1, None, None));
        let grants: i64 = tx.query_row(
            "SELECT count(*) FROM role_permissions WHERE role_id = 'role_owner'", [], |row| row.get(0),
        )?;
        assert_eq!(usize::try_from(grants).expect("count"), Permission::ALL.len());
        Ok(())
    }).expect("owner sync");
}

#[test]
fn owner_permissions_do_not_widen_a_custom_role_named_owner() {
    let scratch = Scratch::new("owner_permissions_custom");
    let db = scratch.open();
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        assert!(repos.people().apply_role_from_cloud(OUTLET, &cloud_role("custom_owner"))?);
        let role = repos.people().list_roles(OUTLET)?.remove(0);
        assert_eq!(role.permissions, [Permission::BillCreate].into_iter().collect());
        assert!(!role.permissions.has(Permission::BillRevert));
        assert_eq!(role.max_discount_bp, Some(0));
        assert_eq!(role.max_discount, Some(Money::ZERO));
        assert!(!role.is_builtin);
        Ok(())
    }).expect("custom role keeps its explicit grants and limits");
}
