use std::{path::PathBuf, sync::Arc};

use image::{imageops::FilterType, GenericImageView, RgbaImage};
use parking_lot::Mutex;
use wild_doc_script::{
    anyhow::Result, async_trait, IncludeAdaptor, Stack, WildDocScript, WildDocValue,
};

pub struct WdImage {}

fn var2u32(value: &str, stack: &Stack) -> Option<u32> {
    if value.starts_with("$") {
        let value = unsafe { std::str::from_utf8_unchecked(&value.as_bytes()[1..]) };
        if let Some(WildDocValue::Number(v)) = stack.get(value) {
            if let Some(v) = v.as_u64() {
                return Some(v as u32);
            }
        }
    } else {
        if let Ok(v) = value.parse::<u32>() {
            return Some(v);
        }
    }
    None
}

#[async_trait(?Send)]
impl WildDocScript for WdImage {
    fn new(_: Arc<Mutex<Box<dyn IncludeAdaptor + Send>>>, _: PathBuf, _: &Stack) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {})
    }

    async fn evaluate_module(&mut self, _: &str, _: &str, _: &Stack) -> Result<()> {
        Ok(())
    }

    async fn eval(&mut self, code: &str, stack: &Stack) -> Result<WildDocValue> {
        let splited: Vec<&str> = code.split("?").collect();
        if let Some(image_var) = splited.get(0) {
            if let Some(WildDocValue::Binary(image)) = stack.get(image_var) {
                if let Some(param) = splited.get(1) {
                    if let Ok(mut image) = image::load_from_memory(image) {
                        let mut w = None;
                        let mut h = None;
                        let mut mode: Option<&str> = None;
                        for p in param.split("&").into_iter() {
                            let pp: Vec<&str> = p.split("=").collect();
                            if pp.len() == 2 {
                                let value = unsafe { pp.get_unchecked(1) };
                                match unsafe { pp.get_unchecked(0).as_ref() } {
                                    "w" => {
                                        w = var2u32(value, stack);
                                    }
                                    "h" => {
                                        h = var2u32(value, stack);
                                    }
                                    "m" => {
                                        if value.starts_with("$") {
                                            if let Some(WildDocValue::String(v)) =
                                                stack.get(unsafe {
                                                    std::str::from_utf8_unchecked(
                                                        &value.as_bytes()[1..],
                                                    )
                                                })
                                            {
                                                mode = Some(v);
                                            }
                                        } else {
                                            mode = Some(value);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if w.is_some() && h.is_none() {
                            let r = image.width() as f32 / w.unwrap() as f32;
                            h = Some((image.height() as f32 * r) as u32);
                        } else if w.is_none() && h.is_some() {
                            let r = image.height() as f32 / h.unwrap() as f32;
                            w = Some((image.width() as f32 * r) as u32);
                        }
                        if let (Some(w), Some(h)) = (w, h) {
                            image = match mode {
                                Some("fit") => image.resize(w, h, FilterType::Triangle),
                                Some("cover") => image.resize_to_fill(w, h, FilterType::Triangle),
                                Some("contain") => {
                                    let resized = image.resize(w, h, FilterType::Triangle);
                                    let resized_w = resized.width();
                                    let resized_h = resized.height();
                                    if resized_w != w || resized_h != h {
                                        let mut image = RgbaImage::new(w, h);
                                        let offset_x = (w - resized_w) / 2;
                                        let offset_y = (h - resized_h) / 2;
                                        for y in 0..resized_h {
                                            for x in 0..resized_w {
                                                image.put_pixel(
                                                    x + offset_x,
                                                    y + offset_y,
                                                    resized.get_pixel(x, y),
                                                );
                                            }
                                        }
                                        image.into()
                                    } else {
                                        resized
                                    }
                                }
                                Some("crop") => image.crop_imm(
                                    (image.width() - w) / 2,
                                    (image.height() - h) / 2,
                                    w,
                                    h,
                                ),
                                _ => image.resize_exact(w, h, FilterType::Triangle),
                            };
                        }
                        let mut buffer = std::io::Cursor::new(vec![]);
                        let _ = image.write_to(&mut buffer, image::ImageOutputFormat::WebP);
                        return Ok(WildDocValue::Binary(buffer.into_inner()));
                    }
                } else {
                    return Ok(WildDocValue::Binary(image.clone()));
                }
            }
        }
        Ok(WildDocValue::Null)
    }
}
