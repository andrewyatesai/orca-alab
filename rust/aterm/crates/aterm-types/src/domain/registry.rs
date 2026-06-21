// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Domain registry for managing multiple domains.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use super::{Domain, DomainId, DomainType};

/// Domain registry for managing multiple domains.
#[derive(Default)]
pub struct DomainRegistry {
    domains: RwLock<HashMap<DomainId, Arc<dyn Domain>>>,
    default_domain: RwLock<Option<DomainId>>,
}

impl DomainRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a domain.
    pub fn register(&self, domain: Arc<dyn Domain>) {
        let id = domain.domain_id();
        let mut domains = self.domains.write().unwrap_or_else(PoisonError::into_inner);
        let mut default = self
            .default_domain
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        domains.insert(id, domain);
        if default.is_none() {
            *default = Some(id);
        }
    }

    /// Unregister a domain.
    #[must_use]
    pub fn unregister(&self, id: DomainId) -> Option<Arc<dyn Domain>> {
        let mut domains = self.domains.write().unwrap_or_else(PoisonError::into_inner);
        let mut default = self
            .default_domain
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let domain = domains.remove(&id);
        if *default == Some(id) {
            *default = domains.keys().next().copied();
        }
        domain
    }

    /// Get a domain by ID.
    #[must_use]
    pub fn get(&self, id: DomainId) -> Option<Arc<dyn Domain>> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
    }

    /// Get the default domain.
    #[must_use]
    pub fn default_domain(&self) -> Option<Arc<dyn Domain>> {
        let default = self
            .default_domain
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        default.and_then(|id| self.get(id))
    }

    /// Set the default domain.
    pub fn set_default(&self, id: DomainId) {
        let mut default = self
            .default_domain
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *default = Some(id);
    }

    /// List all registered domains.
    #[must_use]
    pub fn list(&self) -> Vec<Arc<dyn Domain>> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// List registered domains of a given type.
    #[must_use]
    pub fn list_by_type(&self, domain_type: DomainType) -> Vec<Arc<dyn Domain>> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|d| d.domain_type() == domain_type)
            .cloned()
            .collect()
    }

    /// List domains that advertise remote execution.
    #[must_use]
    pub fn list_remote(&self) -> Vec<Arc<dyn Domain>> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|d| d.capabilities().remote)
            .cloned()
            .collect()
    }

    /// List domains that advertise pane multiplexing.
    #[must_use]
    pub fn list_multiplexers(&self) -> Vec<Arc<dyn Domain>> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|d| d.capabilities().multiplexed)
            .cloned()
            .collect()
    }

    /// Get a domain by name.
    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn Domain>> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|d| d.domain_name() == name)
            .cloned()
    }
}
