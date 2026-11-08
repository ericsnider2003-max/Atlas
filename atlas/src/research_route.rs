//! Small, dependency-free research model routing policy.
//!
//! Keeping this decision separate from the daemon makes the most important
//! hardware-boundary rule testable without linking Atlas's desktop, browser,
//! or ONNX stacks: a delegated writer may operate without a local model only
//! when local verification was explicitly disabled.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Writer {
    Local,
    Delegated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) writer: Writer,
    pub(crate) verify_locally: bool,
    pub(crate) fallback_locally: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    NoWriter,
    MissingLocalVerifier,
}

/// Select the research writer without constructing or loading any model.
pub(crate) fn plan(
    delegated_available: bool,
    local_available: bool,
    verify_requested: bool,
) -> Result<Plan, Error> {
    if !delegated_available && !local_available {
        return Err(Error::NoWriter);
    }
    if delegated_available && verify_requested && !local_available {
        return Err(Error::MissingLocalVerifier);
    }
    if delegated_available {
        return Ok(Plan {
            writer: Writer::Delegated,
            verify_locally: verify_requested,
            fallback_locally: local_available,
        });
    }
    Ok(Plan {
        writer: Writer::Local,
        verify_locally: false,
        fallback_locally: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{plan, Error, Plan, Writer};

    #[test]
    fn delegated_writer_can_run_without_local_model_when_verification_is_off() {
        assert_eq!(
            plan(true, false, false),
            Ok(Plan {
                writer: Writer::Delegated,
                verify_locally: false,
                fallback_locally: false,
            })
        );
    }

    #[test]
    fn delegated_writer_requires_local_verifier_by_default() {
        assert_eq!(plan(true, false, true), Err(Error::MissingLocalVerifier));
    }

    #[test]
    fn delegated_writer_prefers_local_fallback_when_available() {
        assert_eq!(
            plan(true, true, false),
            Ok(Plan {
                writer: Writer::Delegated,
                verify_locally: false,
                fallback_locally: true,
            })
        );
    }

    #[test]
    fn local_writer_does_not_need_verification_or_fallback() {
        assert_eq!(
            plan(false, true, true),
            Ok(Plan {
                writer: Writer::Local,
                verify_locally: false,
                fallback_locally: false,
            })
        );
    }

    #[test]
    fn no_writer_is_an_explicit_failure() {
        assert_eq!(plan(false, false, false), Err(Error::NoWriter));
    }
}
