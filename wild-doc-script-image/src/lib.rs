use std::{path::PathBuf, sync::Arc};

use image::{GenericImageView, RgbaImage, imageops::FilterType};
use parking_lot::Mutex;
use wild_doc_script::{
    IncludeAdaptor, Stack, WildDocScript, WildDocValue, anyhow::Result, async_trait,
};

pub struct WdImage {}

fn var2u32(value: &str, stack: &Stack) -> Option<u32> {
    if let Some(value) = value.strip_prefix('$') {
        if let Some(WildDocValue::Number(v)) = stack.get(&Arc::new(value.into())) {
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
impl<I: IncludeAdaptor + Send> WildDocScript<I> for WdImage {
    fn new(_: Arc<Mutex<I>>, _: PathBuf, _: &Stack) -> Result<Self>
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
        if let Some(image_var) = splited.first() {
            if let Some(WildDocValue::Binary(image)) = stack.get(&Arc::new(image_var.to_string())) {
                if let Some(param) = splited.get(1) {
                    if let Ok(mut image) = image::load_from_memory(image) {
                        let mut w = None;
                        let mut h = None;
                        let mut mode: Option<&str> = None;
                        for p in param.split('&') {
                            let pp: Vec<&str> = p.split('=').collect();
                            if let [key, value] = pp.as_slice() {
                                match *key {
                                    "w" => {
                                        w = var2u32(value, stack);
                                    }
                                    "h" => {
                                        h = var2u32(value, stack);
                                    }
                                    "m" => {
                                        if let Some(var) = value.strip_prefix('$') {
                                            if let Some(WildDocValue::String(v)) =
                                                stack.get(&Arc::new(var.to_owned()))
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
                        // keep the aspect ratio when only one side is given.
                        let (width, height) = (image.width() as f32, image.height() as f32);
                        match (w, h) {
                            (Some(w), None) if width > 0.0 => {
                                h = Some((height * w as f32 / width).round() as u32);
                            }
                            (None, Some(h)) if height > 0.0 => {
                                w = Some((width * h as f32 / height).round() as u32);
                            }
                            _ => {}
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
                                    image.width().saturating_sub(w) / 2,
                                    image.height().saturating_sub(h) / 2,
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

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use image::{DynamicImage, GenericImageView, ImageOutputFormat};
    use wild_doc_script::{IncludeAdaptor, Stack, Vars, WildDocScript, WildDocValue};

    use super::WdImage;

    struct NoInclude;
    impl IncludeAdaptor for NoInclude {
        fn include(&mut self, _: &Path) -> Option<Arc<Vec<u8>>> {
            None
        }
    }

    fn resize(code: &str) -> (u32, u32) {
        let mut png = std::io::Cursor::new(vec![]);
        DynamicImage::new_rgba8(200, 100)
            .write_to(&mut png, ImageOutputFormat::Png)
            .unwrap();
        let vars: Vars = [(
            Arc::new("img".to_owned()),
            WildDocValue::Binary(png.into_inner()),
        )]
        .into_iter()
        .collect();
        let stack = Stack::new(Some(vars));

        let mut wd_image = WdImage {};
        let value = futures::executor::block_on(<WdImage as WildDocScript<NoInclude>>::eval(
            &mut wd_image,
            code,
            &stack,
        ))
        .unwrap();
        let WildDocValue::Binary(bytes) = value else {
            panic!("not binary");
        };
        image::load_from_memory(&bytes).unwrap().dimensions()
    }

    #[test]
    fn keeps_aspect_ratio_when_one_side_is_given() {
        assert_eq!(resize("img?w=100"), (100, 50));
        assert_eq!(resize("img?h=25"), (50, 25));
        assert_eq!(resize("img?w=100&h=100"), (100, 100));
    }

    #[test]
    fn crop_larger_than_the_image_does_not_panic() {
        let (w, h) = resize("img?w=300&h=300&m=crop");
        assert!(w <= 300 && h <= 300);
    }
}
