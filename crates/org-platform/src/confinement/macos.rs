use crate::Refused;

pub(super) fn supported() -> Result<(), Refused> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn macos_supports_the_web_process() {
        super::super::web_confinement_supported().unwrap();
    }
}
