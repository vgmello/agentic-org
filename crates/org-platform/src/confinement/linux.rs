//! GNU/Linux has no web confinement adapter yet. Increment X4 writes it (Landlock and
//! seccomp, Linux host design section 3.3). Until then every function is the refusing
//! adapter's, so Linux refuses exactly as a target without an adapter does.

pub(super) use super::refused::supported;

#[cfg(test)]
mod tests {
    use crate::Refused;

    #[test]
    fn linux_refuses_until_its_adapter_is_written() {
        assert_eq!(
            super::super::web_confinement_supported(),
            Err(Refused::unsupported("web process confinement"))
        );
    }
}
