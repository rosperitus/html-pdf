//! WebAssembly bindings: `render(html, options)` for Node.js and browsers.

use html_pdf::{Font, Options};
use js_sys::{Array, ArrayBuffer, Function, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** Font or image bytes: a Uint8Array (Node.js Buffer included) or an ArrayBuffer. */
export type Bytes = Uint8Array | ArrayBuffer;

export interface RenderOptions {
  /**
   * TrueType (.ttf) fonts. CSS font-family picks among them by family name,
   * weight and style; unknown families, generic ones included, use the first font.
   */
  fonts: Bytes[];
  /** Page width and height in points (1/72 in). Default A4. `@page { size }` in the document wins. */
  pageSize?: [number, number];
  /**
   * Margins in points: one number for all sides, or [top, right, bottom, left].
   * Default 1 cm. `@page { margin }` in the document wins.
   */
  margin?: number | [number, number, number, number];
  /**
   * Returns the bytes behind an <img src> that is not a data: URI, or undefined
   * to skip the image. Called synchronously: fetch images beforehand.
   */
  resources?: (src: string) => Bytes | undefined | null;
}

/** Renders HTML to PDF bytes. Throws if a font cannot be used. */
export function render(html: string, options: RenderOptions): Uint8Array;
"#;

fn bytes(v: JsValue) -> Option<Vec<u8>> {
    if let Some(a) = v.dyn_ref::<Uint8Array>() {
        Some(a.to_vec())
    } else {
        v.dyn_ref::<ArrayBuffer>()
            .map(|b| Uint8Array::new(b).to_vec())
    }
}

/// Numbers from a number or an array of numbers; `None` for anything else.
fn numbers(v: &JsValue) -> Option<Vec<f32>> {
    if Array::is_array(v) {
        Array::from(v)
            .iter()
            .map(|x| x.as_f64().map(|n| n as f32))
            .collect()
    } else {
        v.as_f64().map(|n| vec![n as f32])
    }
}

#[wasm_bindgen(skip_typescript)]
pub fn render(html: &str, options: &JsValue) -> Result<Vec<u8>, JsError> {
    let get =
        |key: &str| Reflect::get(options, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED);

    let fonts = get("fonts");
    if !Array::is_array(&fonts) {
        return Err(JsError::new(
            "options.fonts must be an array of Uint8Array or ArrayBuffer",
        ));
    }
    let data: Vec<Vec<u8>> = Array::from(&fonts)
        .iter()
        .map(|f| {
            bytes(f).ok_or_else(|| {
                JsError::new("options.fonts must hold Uint8Array or ArrayBuffer values")
            })
        })
        .collect::<Result<_, _>>()?;
    let fonts = data
        .iter()
        .map(|d| Font::new(d))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| JsError::new(&e.to_string()))?;
    let mut opts = Options::new(fonts);

    let page_size = get("pageSize");
    if !page_size.is_undefined() {
        match numbers(&page_size).as_deref() {
            Some(&[w, h]) => opts.page_size = (w, h),
            _ => {
                return Err(JsError::new(
                    "options.pageSize must be [width, height] in points",
                ));
            }
        }
    }
    let margin = get("margin");
    if !margin.is_undefined() {
        opts.margin = match numbers(&margin).as_deref() {
            Some(&[m]) => [m; 4],
            Some(&[t, r, b, l]) => [t, r, b, l],
            _ => {
                return Err(JsError::new(
                    "options.margin must be a number or [top, right, bottom, left]",
                ));
            }
        };
    }
    if let Ok(f) = get("resources").dyn_into::<Function>() {
        opts.resources = Some(Box::new(move |src: &str| {
            bytes(f.call1(&JsValue::NULL, &JsValue::from_str(src)).ok()?)
        }));
    }

    html_pdf::render(html, &opts).map_err(|e| JsError::new(&e.to_string()))
}
