//! Roles and the central permission rules (docs/product/roles-and-permissions.md).

/// Instance roles, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    RequirementManager,
    Requester,
}

impl Role {
    /// May make human review decisions: Accept / Request Changes / Reject / Reopen.
    pub fn can_review(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::RequirementManager)
    }

    /// May administer the instance: repositories, daemon settings, user management.
    pub fn can_administer(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
}

/// Why a role assignment was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleError {
    /// Actor lacks permission to assign roles at all.
    NotAuthorized,
    /// Only an Owner may grant Owner; Admin cannot.
    OwnerGrantRequiresOwner,
    /// Nobody may change their own role (no self-promotion).
    SelfModification,
}

/// Canonical rule for changing another user's role:
///
/// - nobody modifies their own role;
/// - Owner may assign any role;
/// - Admin may assign everything except Owner;
/// - everyone else assigns nothing.
///
/// Every normal new account starts as [`Role::Requester`]. The first account on a
/// fresh instance becomes [`Role::Owner`] atomically in persistence (see
/// docs/architecture/persistence.md); this function never grants initial ownership.
pub fn assign_role(actor: Role, actor_is_target: bool, new_role: Role) -> Result<(), RoleError> {
    if actor_is_target {
        return Err(RoleError::SelfModification);
    }
    match actor {
        Role::Owner => Ok(()),
        Role::Admin if new_role != Role::Owner => Ok(()),
        Role::Admin => Err(RoleError::OwnerGrantRequiresOwner),
        _ => Err(RoleError::NotAuthorized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_and_administration_matrices_hold() {
        assert!(
            Role::Owner.can_review()
                && Role::Admin.can_review()
                && Role::RequirementManager.can_review()
        );
        assert!(!Role::Requester.can_review());
        assert!(Role::Owner.can_administer() && Role::Admin.can_administer());
        assert!(!Role::RequirementManager.can_administer());
        assert!(!Role::Requester.can_administer());
    }

    #[test]
    fn assignment_rules_hold() {
        use Role::*;
        assert_eq!(assign_role(Owner, false, Admin), Ok(()));
        assert_eq!(assign_role(Admin, false, RequirementManager), Ok(()));
        assert_eq!(
            assign_role(Admin, false, Owner),
            Err(RoleError::OwnerGrantRequiresOwner)
        );
        assert_eq!(
            assign_role(Admin, true, Requester),
            Err(RoleError::SelfModification)
        );
        assert_eq!(
            assign_role(Owner, true, Requester),
            Err(RoleError::SelfModification)
        );
        assert_eq!(
            assign_role(RequirementManager, false, Requester),
            Err(RoleError::NotAuthorized)
        );
        assert_eq!(
            assign_role(Requester, false, Admin),
            Err(RoleError::NotAuthorized)
        );
    }
}
