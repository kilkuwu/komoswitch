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
    pub maximized: COLORREF,
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
            true => COLORREF::from_rgb(0, 0, 0), 
            false => COLORREF::from_rgb(255, 255, 255), 
        };
        let focused = match is_light_mode {
            true => ui_settings.GetColorValue(UIColorType::AccentDark1)?,
            false => ui_settings.GetColorValue(UIColorType::AccentLight2)?,
        };
        let focused = COLORREF::from_rgb(focused.R, focused.G, focused.B);
        let nonempty = match is_light_mode {
            true => COLORREF::from_rgb(150, 150, 150),
            false => COLORREF::from_rgb(100, 100, 100),
        };

        let empty = match is_light_mode {
            true => COLORREF::from_rgb(200, 200, 200), 
            false => COLORREF::from_rgb(50, 50, 50),  
        };

        let monocle = match is_light_mode {
            true => COLORREF::from_rgb(255, 135, 210),
            false => COLORREF::from_rgb(225, 21, 123), 
        };

        let maximized = match is_light_mode {
            true => COLORREF::from_rgb(180, 215, 215),
            false => COLORREF::from_rgb(10, 102, 194),
        };

        Ok(Self {
            nonempty,
            focused,
            empty,
            monocle,
            maximized,
            foreground,
        })
    }
}

pub struct Settings {
    pub colors: ColorSettings,
    pub font: HFONT,
    pub transparent_brush: HBRUSH,
    pub transparent_pen: HPEN,
}

impl Settings {
    pub fn new() -> anyhow::Result<Settings> {
        let colors = ColorSettings::new()?;
        let mut lf = LOGFONT::default();
        lf.lfHeight = 24;
        if colors.is_light_mode() {
            lf.set_lfFaceName("Segoe UI Variable Text Semibold");
        } else {
            lf.set_lfFaceName("Segoe UI Variable Text");
        }
        let font = HFONT::CreateFontIndirect(&lf)?.leak();
        let transparent_brush = HBRUSH::CreateSolidBrush(colors.get_color_key())?.leak();
        let transparent_pen = HPEN::CreatePen(co::PS::SOLID, 1, colors.get_color_key())?.leak();

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
