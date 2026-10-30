//! Which items count, and when one is documented: the scan's settings, which features fill in.

use crate::Scope;

/// Which items count, and when one is documented. The default is docov's out of the box.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    /// Which declarations count.
    pub scope: Scope,
    /// Whether a JSDoc block documents its item only when it describes it, not with tags alone.
    pub require_description: bool,
}

impl From<Scope> for Rules {
    fn from(scope: Scope) -> Self {
        Self { scope, ..Self::default() }
    }
}
