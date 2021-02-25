use core::{cell::Cell, mem};
use wasm_bindgen::prelude::*;

#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

const MIN_FRAMERATE: f64 = 60.0;
const MIN_TABULATION_SIZE: usize = 257;

fn calc_tabulation_size(a: f64, l: f64) -> usize {
    MIN_TABULATION_SIZE.max((2.0 * l / a * MIN_FRAMERATE + 1.0).ceil() as usize)
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(arg: number) => number")]
    pub type RealFunction;

    #[wasm_bindgen(method, js_name = "call")]
    pub fn call_impl(f: &RealFunction, this: JsValue, arg: f64) -> f64;

    // #[wasm_bindgen(js_namespace = console)]
    // pub fn log(msg: &str);
}

impl RealFunction {
    fn call(&self, arg: f64) -> f64 {
        self.call_impl(JsValue::NULL, arg)
    }
}

#[wasm_bindgen]
pub struct CurveView {
    visible: bool,
    color: JsValue,
}

#[wasm_bindgen]
impl CurveView {
    #[wasm_bindgen(constructor)]
    pub fn new(visible: bool, color: JsValue) -> CurveView {
        CurveView { visible, color }
    }
}

#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub enum UDiffType {
    Ut = "Ut",
    Ux = "Ux",
}

#[wasm_bindgen]
pub struct UDiff {
    pub ty: UDiffType,
    func: RealFunction,
}

#[wasm_bindgen]
impl UDiff {
    #[wasm_bindgen(constructor)]
    pub fn new(ty: UDiffType, func: RealFunction) -> Self {
        UDiff { ty, func }
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct UDiffPairPoint {
    u_t: f64,
    u_x: f64,
}

#[wasm_bindgen]
pub struct Renderer {
    left: UDiff,
    right: UDiff,
    floor: Vec<Cell<UDiffPairPoint>>,
    floor_buffer: Vec<Cell<UDiffPairPoint>>,
    pub a: f64,
    pub l: f64,
    rem_t: f64,
    cur_t: f64,
}

#[wasm_bindgen]
impl Renderer {
    #[wasm_bindgen(constructor)]
    pub fn new(
        left: UDiff,
        right: UDiff,
        bottom_u_x: RealFunction,
        bottom_u_t: RealFunction,
        a: f64,
        l: f64,
    ) -> Renderer {
        let floor = Self::generate_floor(bottom_u_x, bottom_u_t, a, l);
        let mut floor_buffer = Vec::new();
        floor_buffer.resize(floor.len(), Cell::default());
        Self {
            floor_buffer,
            rem_t: 0.0,
            cur_t: 0.0,
            left,
            right,
            floor,
            a,
            l,
        }
    }

    pub fn reset(&mut self, bottom_u_x: RealFunction, bottom_u_t: RealFunction, a: f64, l: f64) {
        self.a = a;
        self.l = l;
        self.floor = Self::generate_floor(bottom_u_x, bottom_u_t, a, l);
        self.floor_buffer.resize(self.floor.len(), Cell::default());
    }

    pub fn advance(&mut self, dt: f64) {
        let step_dt = self.step_dt();
        self.rem_t = dt % step_dt;
        if let None = self.nth((dt / step_dt) as _) {
            unreachable!()
        }
    }

    fn step_dt(&self) -> f64 {
        2.0 * self.l / (self.a * (self.floor.len() - 1) as f64)
    }

    fn calc_point(&self, left: UDiffPairPoint, right: UDiffPairPoint) -> UDiffPairPoint {
        let a = self.a;
        UDiffPairPoint {
            u_x: (left.u_x - left.u_t / a + right.u_x + right.u_t / a) / 2.0,
            u_t: (left.u_t - left.u_x * a + right.u_t + right.u_x * a) / 2.0,
        }
    }

    fn left_calc_point(&self, t: f64, point: UDiffPairPoint) -> UDiffPairPoint {
        let u_diff = self.left.func.call(t);
        match self.left.ty {
            UDiffType::Ut => {
                let u_t = u_diff;
                UDiffPairPoint {
                    u_x: point.u_x - (u_t - point.u_t) / self.a,
                    u_t,
                }
            }
            UDiffType::Ux => {
                let u_x = u_diff;
                UDiffPairPoint {
                    u_t: point.u_t - (u_x - point.u_x) * self.a,
                    u_x,
                }
            }
            _ => unreachable!(),
        }
    }

    fn right_calc_point(&self, t: f64, point: UDiffPairPoint) -> UDiffPairPoint {
        let u_diff = self.right.func.call(t);
        match self.right.ty {
            UDiffType::Ut => {
                let u_t = u_diff;
                UDiffPairPoint {
                    u_x: point.u_x + (u_t - point.u_t) / self.a,
                    u_t,
                }
            }
            UDiffType::Ux => {
                let u_x = u_diff;
                UDiffPairPoint {
                    u_t: point.u_t + (u_x - point.u_x) * self.a,
                    u_x,
                }
            }
            _ => unreachable!(),
        }
    }

    pub fn render_canvas(
        &self,
        ctx: &web_sys::CanvasRenderingContext2d,
        u_view: CurveView,
        u_x_view: CurveView,
        u_t_view: CurveView,
    ) -> Result<(), JsValue> {
        const CANVAS_WIDTH: u32 = 480;
        const CANVAS_HEIGHT: u32 = 480;

        ctx.clear_rect(0.0, 0.0, CANVAS_WIDTH as f64, CANVAS_HEIGHT as f64);

        let n = self.floor.len();

        let x_from_idx = |i| CANVAS_WIDTH as f64 * i as f64 / (n - 1) as f64;
        let t_y = |y| CANVAS_HEIGHT as f64 * (0.5 - y / self.l);

        let u_x_path = web_sys::Path2d::new()?;
        let u_t_path = web_sys::Path2d::new()?;

        let init_point = self.floor.first().unwrap();
        u_x_path.move_to(0.0, t_y(init_point.get().u_x));
        u_t_path.move_to(0.0, t_y(init_point.get().u_t));

        self.floor.iter().enumerate().skip(1).for_each(|(i, p)| {
            u_x_path.line_to(x_from_idx(i), t_y(p.get().u_x));
            u_t_path.line_to(x_from_idx(i), t_y(p.get().u_t));
        });

        if u_x_view.visible {
            ctx.set_stroke_style(&u_x_view.color);
            ctx.stroke_with_path(&u_x_path);
        }
        if u_t_view.visible {
            ctx.set_stroke_style(&u_t_view.color);
            ctx.stroke_with_path(&u_t_path);
        }

        Ok(())
    }

    #[wasm_bindgen(setter)]
    pub fn set_left_ty(&mut self, ty: UDiffType) {
        self.left.ty = ty;
    }

    #[wasm_bindgen(setter)]
    pub fn set_right_ty(&mut self, ty: UDiffType) {
        self.right.ty = ty;
    }

    #[wasm_bindgen(setter)]
    pub fn set_left_func(&mut self, func: RealFunction) {
        self.left.func = func;
    }

    #[wasm_bindgen(setter)]
    pub fn set_right_func(&mut self, func: RealFunction) {
        self.right.func = func;
    }

    fn generate_floor(
        bottom_u_x: RealFunction,
        bottom_u_t: RealFunction,
        a: f64,
        l: f64,
    ) -> Vec<Cell<UDiffPairPoint>> {
        let n = calc_tabulation_size(a, l);

        (0..n)
            .map(|i| {
                let x = i as f64 / (n - 1) as f64 * l;
                Cell::new(UDiffPairPoint {
                    u_t: bottom_u_t.call(x),
                    u_x: bottom_u_x.call(x),
                })
            })
            .collect()
    }
}

impl Iterator for Renderer {
    type Item = ();

    fn next(&mut self) -> Option<()> {
        self.cur_t += self.step_dt();

        let n = self.floor.len();
        debug_assert_eq!(n, self.floor_buffer.len());

        self.floor_buffer[0].replace(self.left_calc_point(self.cur_t, self.floor[1].get()));
        self.floor_buffer[1..n - 1]
            .iter()
            .zip(self.floor.iter().zip(self.floor.iter().skip(2)))
            .for_each(|(out_point, (a, b))| {
                out_point.replace(self.calc_point(a.get(), b.get()));
            });
        self.floor_buffer[n - 1]
            .replace(self.right_calc_point(self.cur_t, self.floor[n - 2].get()));

        mem::swap(&mut self.floor, &mut self.floor_buffer);
        Some(())
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();
}
