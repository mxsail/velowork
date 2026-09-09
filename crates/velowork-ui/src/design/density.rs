pub use velowork_core::theme::UiDensity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_density_offsets() {
        assert_eq!(UiDensity::Compact.height_offset(), -4.0);
        assert_eq!(UiDensity::Default.height_offset(), 0.0);
        assert_eq!(UiDensity::Comfortable.height_offset(), 4.0);
    }

    #[test]
    fn test_density_spacing_factors() {
        assert_eq!(UiDensity::Compact.spacing_factor(), 0.80);
        assert_eq!(UiDensity::Default.spacing_factor(), 1.00);
        assert_eq!(UiDensity::Comfortable.spacing_factor(), 1.25);
    }

    #[test]
    fn test_default_density() {
        let d: UiDensity = Default::default();
        assert_eq!(d, UiDensity::Default);
    }
}
