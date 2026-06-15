// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Extension registry with a trust-tier gate (ATERM_DESIGN WS-D).
//!
//! Extensions are named handlers the host can dispatch to. Each declares the
//! minimum trust [`Tier`] a caller must hold to invoke it, and dispatch is gated
//! on a [`Cap<InvokeExtension>`] from [`aterm_cap`]: a caller cannot invoke an
//! extension above its capability's tier, and cannot invoke ANY extension
//! without a capability at all (the cap is required by type). This is the
//! "registry + trust-tier gate" the design calls for — capability-first, so
//! authorization is structural, not a runtime flag the extension could bypass.
//!
//! STATUS (per §0.1): the gate and dispatch are tested; the cap cannot be
//! struct-literal-forged outside `aterm-cap` (its no-struct-forgery guarantee).
//! The stronger no-mint-reachability property is `aterm-cap`'s ROADMAP §5.4 work,
//! NOT yet delivered — see that crate's docs for the exact, honest scope.

use std::collections::BTreeMap;

use aterm_cap::{Cap, Tier};

/// The effect a capability authorizes here: invoking a registered extension.
pub enum InvokeExtension {}

/// A named extension handler.
pub trait Extension: Send + Sync {
    /// The dispatch key.
    fn name(&self) -> &str;

    /// Minimum trust tier a caller must present to invoke this extension.
    /// Defaults to `Trusted`.
    fn required_tier(&self) -> Tier {
        Tier::Trusted
    }

    /// Handle a request, returning a response. Only reached after the tier gate
    /// has passed.
    fn handle(&self, request: &str) -> String;
}

/// Why a dispatch did not run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchError {
    /// No extension is registered under that name.
    NotFound,
    /// The caller's capability tier is below the extension's requirement.
    Denied { have: Tier, need: Tier },
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::NotFound => write!(f, "no such extension"),
            DispatchError::Denied { have, need } => {
                write!(f, "extension denied: have {have:?}, need at least {need:?}")
            }
        }
    }
}
impl std::error::Error for DispatchError {}

/// A registry of trust-tier-gated extensions.
#[derive(Default)]
pub struct Registry {
    exts: BTreeMap<String, Box<dyn Extension>>,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Registry { exts: BTreeMap::new() }
    }

    /// Register `ext`, replacing any extension already under its name. Returns the
    /// displaced extension, if any.
    pub fn register(&mut self, ext: Box<dyn Extension>) -> Option<Box<dyn Extension>> {
        self.exts.insert(ext.name().to_string(), ext)
    }

    /// The registered extension names, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.exts.keys().map(String::as_str).collect()
    }

    /// Whether an extension is registered under `name`.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.exts.contains_key(name)
    }

    /// Dispatch `request` to the extension `name`, but ONLY if `cap`'s tier meets
    /// the extension's `required_tier`. The `&Cap<InvokeExtension>` is mandatory:
    /// without it there is no way to call this, and it cannot be forged.
    ///
    /// # Errors
    /// [`DispatchError::NotFound`] if no such extension; [`DispatchError::Denied`]
    /// if the capability tier is insufficient.
    pub fn dispatch(
        &self,
        name: &str,
        request: &str,
        cap: &Cap<InvokeExtension>,
    ) -> Result<String, DispatchError> {
        let ext = self.exts.get(name).ok_or(DispatchError::NotFound)?;
        let need = ext.required_tier();
        if !cap.satisfies(need) {
            return Err(DispatchError::Denied { have: cap.tier(), need });
        }
        Ok(ext.handle(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_cap::Authority;

    struct Echo;
    impl Extension for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn required_tier(&self) -> Tier {
            Tier::Untrusted
        }
        fn handle(&self, request: &str) -> String {
            format!("echo:{request}")
        }
    }

    struct Privileged;
    impl Extension for Privileged {
        fn name(&self) -> &str {
            "privileged"
        }
        fn required_tier(&self) -> Tier {
            Tier::Certified
        }
        fn handle(&self, _request: &str) -> String {
            "did-privileged-thing".to_string()
        }
    }

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.register(Box::new(Echo));
        r.register(Box::new(Privileged));
        r
    }

    #[test]
    fn registers_and_lists() {
        let r = registry();
        assert_eq!(r.names(), vec!["echo", "privileged"]);
        assert!(r.contains("echo"));
        assert!(!r.contains("nope"));
    }

    #[test]
    fn dispatch_allows_sufficient_tier() {
        let r = registry();
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<InvokeExtension> = auth.grant(Tier::Trusted);
        // echo needs only Untrusted -> allowed.
        assert_eq!(r.dispatch("echo", "hi", &cap), Ok("echo:hi".to_string()));
    }

    #[test]
    fn dispatch_denies_insufficient_tier() {
        let r = registry();
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<InvokeExtension> = auth.grant(Tier::Trusted);
        // privileged needs Certified; a Trusted cap is denied.
        assert_eq!(
            r.dispatch("privileged", "x", &cap),
            Err(DispatchError::Denied { have: Tier::Trusted, need: Tier::Certified })
        );
        // With a Certified cap it goes through.
        let cap2: Cap<InvokeExtension> = auth.grant(Tier::Certified);
        assert_eq!(r.dispatch("privileged", "x", &cap2), Ok("did-privileged-thing".to_string()));
    }

    #[test]
    fn dispatch_unknown_is_not_found() {
        let r = registry();
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<InvokeExtension> = auth.grant(Tier::Certified);
        assert_eq!(r.dispatch("ghost", "x", &cap), Err(DispatchError::NotFound));
    }
}
