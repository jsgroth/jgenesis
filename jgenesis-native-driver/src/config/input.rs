use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay};
use sdl2::keyboard::Keycode;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    Positive,
    Negative,
}

impl Display for AxisDirection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Positive => write!(f, "+"),
            Self::Negative => write!(f, "-"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumDisplay)]
pub enum HatDirection {
    Up,
    Left,
    Right,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoystickAction {
    Button { button_idx: u8 },
    Axis { axis_idx: u8, direction: AxisDirection },
    Hat { hat_idx: u8, direction: HatDirection },
}

impl Display for JoystickAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button { button_idx } => write!(f, "Button {button_idx}"),
            Self::Axis { axis_idx, direction } => write!(f, "Axis {axis_idx} {direction}"),
            Self::Hat { hat_idx, direction } => write!(f, "Hat {hat_idx} {direction}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct JoystickDeviceId {
    pub name: String,
    pub idx: u32, // Used to disambiguate if multiple controllers with the same name are connected
}

impl Display for JoystickDeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} #{}", self.name, self.idx)
    }
}

#[derive(Debug, Clone)]
pub struct JoystickInput {
    pub device: JoystickDeviceId,
    pub action: JoystickAction,
}

impl Display for JoystickInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.action, self.device)
    }
}

#[derive(Debug, Clone)]
pub struct KeyboardInput {
    pub keycode: String,
}

impl Display for KeyboardInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.keycode)
    }
}

#[derive(Debug, Clone)]
pub struct SmsGgControllerConfig<Input> {
    pub up: Option<Input>,
    pub left: Option<Input>,
    pub right: Option<Input>,
    pub down: Option<Input>,
    pub button_1: Option<Input>,
    pub button_2: Option<Input>,
    // Pause is actually shared between the two controllers but it's simpler to map it this way
    pub pause: Option<Input>,
}

impl<Input: Display> Display for SmsGgControllerConfig<Input> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ up: {}, left: {}, right: {}, down: {}, 1: {}, 2: {}, pause: {} }}",
            fmt_option(self.up.as_ref()),
            fmt_option(self.left.as_ref()),
            fmt_option(self.right.as_ref()),
            fmt_option(self.down.as_ref()),
            fmt_option(self.button_1.as_ref()),
            fmt_option(self.button_2.as_ref()),
            fmt_option(self.pause.as_ref())
        )
    }
}

fn fmt_option<T: Display>(option: Option<&T>) -> String {
    match option {
        Some(value) => format!("{value}"),
        None => "<None>".into(),
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgInputConfig<Input> {
    pub p1: SmsGgControllerConfig<Input>,
    pub p2: SmsGgControllerConfig<Input>,
}

impl Default for SmsGgInputConfig<KeyboardInput> {
    fn default() -> Self {
        Self {
            p1: SmsGgControllerConfig {
                up: Some(KeyboardInput { keycode: Keycode::Up.name() }),
                left: Some(KeyboardInput { keycode: Keycode::Left.name() }),
                right: Some(KeyboardInput { keycode: Keycode::Right.name() }),
                down: Some(KeyboardInput { keycode: Keycode::Down.name() }),
                button_1: Some(KeyboardInput { keycode: Keycode::S.name() }),
                button_2: Some(KeyboardInput { keycode: Keycode::A.name() }),
                pause: Some(KeyboardInput { keycode: Keycode::Return.name() }),
            },
            p2: SmsGgControllerConfig {
                up: None,
                left: None,
                right: None,
                down: None,
                button_1: None,
                button_2: None,
                pause: None,
            },
        }
    }
}

impl Default for SmsGgInputConfig<JoystickInput> {
    fn default() -> Self {
        Self {
            p1: SmsGgControllerConfig {
                up: None,
                left: None,
                right: None,
                down: None,
                button_1: None,
                button_2: None,
                pause: None,
            },
            p2: SmsGgControllerConfig {
                up: None,
                left: None,
                right: None,
                down: None,
                button_1: None,
                button_2: None,
                pause: None,
            },
        }
    }
}
