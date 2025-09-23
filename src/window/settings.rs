use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ},
};
use winsafe::*;

pub const TRANSPARENCY_KEY_DARK: COLORREF = COLORREF::from_rgb(0, 0, 0);
pub const TRANSPARENCY_KEY_LIGHT: COLORREF = COLORREF::from_rgb(255, 255, 255);

pub struct ColorSettings {
    pub nonempty: COLORREF,
    pub focused: COLORREF,
    pub empty: COLORREF,
    pub monocle: COLORREF,
    pub foreground: COLORREF,
}

impl ColorSettings {
    pub fn new() -> anyhow::Result<Self> {
        Self::get_colors_from_system()
    }

    pub fn is_light_mode(&self) -> bool {
        self.foreground.GetRValue() == 0
            && self.foreground.GetBValue() == 0
            && self.foreground.GetBValue() == 0
    }

    pub fn get_color_key(&self) -> COLORREF {
        if self.is_light_mode() {
            TRANSPARENCY_KEY_LIGHT
        } else {
            TRANSPARENCY_KEY_DARK
        }
    }

    pub fn get_colors_from_system() -> anyhow::Result<Self> {
        let ui_settings = UISettings::new()?;
        let foreground = ui_settings.GetColorValue(UIColorType::Foreground)?;
        let is_light_mode = foreground.R == 0 && foreground.G == 0 && foreground.B == 0;
        let foreground = match is_light_mode {
            true => COLORREF::from_rgb(0, 0, 0), // black for light mode
            false => COLORREF::from_rgb(255, 255, 255), // white for dark mode
        };
        let focused = match is_light_mode {
            true => ui_settings.GetColorValue(UIColorType::AccentDark1)?,
            false => ui_settings.GetColorValue(UIColorType::AccentLight2)?,
        };
        let focused = COLORREF::from_rgb(focused.R, focused.G, focused.B);
        // let lf = LOGFONT::new_face(24, "Segoe UI Variable Display");
        // let lf = LOGFONT::default();
        let nonempty = match is_light_mode {
            true => COLORREF::from_rgb(150, 150, 150),
            false => COLORREF::from_rgb(100, 100, 100), // green for non-empty workspaces in dark mode
        };

        let empty = match is_light_mode {
            true => COLORREF::from_rgb(200, 200, 200), // light gray for empty workspaces in light mode
            false => COLORREF::from_rgb(50, 50, 50),  // dark gray for empty workspaces in dark mode
        };
        Ok(Self {
            nonempty,
            focused,
            empty,
            monocle: COLORREF::from_rgb(225, 21, 123), // gold for monocle workspace
            foreground,
        })
    }
}

pub struct Settings {
    pub colors: ColorSettings,
    // lf: LOGFONT,
    pub font: HFONT,
    pub transparent_brush: HBRUSH,
    pub transparent_pen: HPEN,
}

impl Settings {
    pub fn new() -> anyhow::Result<Settings> {
        let colors = ColorSettings::new()?;
        // let mut lf = LOGFONT::new_face(0, "Segoe UI Variable Text");
        // // lf.lfOutPrecision = co::OUT_PRECIS::OUTLINE
        // lf.lfQuality = co::QUALITY::CLEARTYPE_NATURAL;
        let mut lf = LOGFONT::default();
        lf.lfHeight = 24;
        if colors.is_light_mode() {
            lf.set_lfFaceName("Segoe UI Variable Text Semibold");
        } else {
            lf.set_lfFaceName("Segoe UI Variable Text");
        }
        // lf.lfQuality = co::QUALITY::NONANTIALIASED;
        let font = HFONT::CreateFontIndirect(&lf)?.leak();
        let transparent_brush = HBRUSH::CreateSolidBrush(colors.get_color_key())?.leak();
        let transparent_pen = HPEN::CreatePen(co::PS::SOLID, 1, colors.get_color_key())?.leak();

        // let lf = LOGFONT::default();
        Ok(Self {
            colors,
            font,
            transparent_brush,
            transparent_pen,
        })
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        unsafe {
            assert!(DeleteObject(HGDIOBJ(self.font.ptr())) != false);
            assert!(DeleteObject(HGDIOBJ(self.transparent_brush.ptr())) != false);
            assert!(DeleteObject(HGDIOBJ(self.transparent_pen.ptr())) != false);
        }
    }
}
