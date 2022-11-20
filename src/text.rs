use std::borrow::Cow;
use std::str::FromStr;
use sfml::graphics::Color;
use sfml::SfBox;
use crate::util::{json_400, JsonError};

#[derive(Debug)]
pub enum Font {
    Impact
}

impl FromStr for Font {
    type Err = JsonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "impact" => Ok(Font::Impact),
            _ => Err(json_400(format!("font: unknown font {}", s)))
        }
    }
}

impl Font {
    pub fn load_sfml(&self) -> SfBox<sfml::graphics::Font> {
        match self {
            Font::Impact => sfml::graphics::Font::from_file("assets/impact.ttf").unwrap()
        }
    }
}

#[derive(Debug)]
pub struct TextParameters {
    pub top_text: Option<String>,
    pub bottom_text: Option<String>,
    pub font: Font,
    pub font_size: u32,
}

impl Default for TextParameters {
    fn default() -> Self {
        TextParameters {
            top_text: None,
            bottom_text: None,
            font: Font::Impact,
            font_size: 32
        }
    }
}

impl TextParameters {
    pub fn set_from_params(&mut self, key: String, value: String) -> Result<(), JsonError> {
        match key.as_str() {
            "top_text" => self.top_text = Some(value),
            "bottom_text" => self.bottom_text = Some(value),
            "font" => self.font = value.parse()?,
            "font_size" => self.font_size = value.parse().map_err(|e| json_400(format!("size: {:?}", e)))?,
            _ => return Err(json_400(Cow::from(format!("Invalid parameter {:?}", key))))
        }
        Ok(())
    }

    pub fn get_text<'font>(&self, font: &'font sfml::graphics::Font, t: &str) -> sfml::graphics::Text<'font> {
        let mut text = sfml::graphics::Text::new(t, font, self.font_size);
        text.set_fill_color(Color::WHITE);
        text.set_outline_color(Color::BLACK);
        text
    }
}
