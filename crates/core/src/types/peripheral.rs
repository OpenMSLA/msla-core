/// Positioning of z-motor
#[derive(Clone, Debug, PartialEq)]
pub enum StepperPositioning {
    /// Absolute positioning relative to physical dimensions
    Absolute,

    /// Relative positioning relative to current motor position
    Relative,
}
