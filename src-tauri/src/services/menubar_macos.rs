//! macOS 原生菜单栏项（NSStatusItem）。
//!
//! 图标 + 两行小字（今日 Token 上行 / 费用下行，9pt 等宽数字字体），
//! 左键唤起主窗口，右键弹原生菜单（显示窗口 / 启用菜单栏显示 / 检查更新 / 退出）。
//!
//! AppKit 状态项相关类型均要求主线程访问，因此所有公开函数都以
//! [`MainThreadMarker`] 为证，非主线程一律通过 `run_on_main_thread` 进入。

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2_core_foundation::{CFRange, CGRect};
use objc2_core_graphics::CGPath;
use objc2_core_text::{CTFrame, CTFramesetter};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBitmapImageRep, NSCellImagePosition, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSEventMask, NSEventType, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSMenu, NSMenuItem,
    NSCompositingOperation, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSStatusBar, NSStatusBarButton,
    NSStatusItem, NSTextAlignment,
};
use objc2_foundation::{
    NSAttributedString, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString,
};

/// 菜单栏小字字号（pt）：上行 Token / 下行费用共用
const MENU_FONT_SIZE: f64 = 9.0;
/// 状态项图标边长（pt）
const MENU_ICON_SIZE: f64 = 18.0;
/// 图标与文字之间的间距（pt）
const MENU_ICON_GAP: f64 = 2.0;
/// 合成图按 2x 渲染，缩放后边缘更干净
const MENU_IMAGE_SCALE: f64 = 2.0;

/// 菜单/点击事件的 Rust 侧回调（lib.rs 注入；触发时已在主线程）
pub struct MenuCallbacks {
    pub show_window: Box<dyn Fn() + Send + Sync>,
    /// 切换"菜单栏显示"开关，返回新状态
    pub toggle_enabled: Box<dyn Fn() -> bool + Send + Sync>,
    pub check_update: Box<dyn Fn() + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

static CALLBACKS: OnceLock<MenuCallbacks> = OnceLock::new();

/// 首次设置文字时打印一次排版几何（偏移量的真机校验）
static GEOMETRY_LOGGED: AtomicBool = AtomicBool::new(false);

// 菜单项 tag
const TAG_SHOW: isize = 0;
const TAG_TOGGLE: isize = 1;
const TAG_UPDATE: isize = 2;
const TAG_QUIT: isize = 3;

thread_local! {
    /// NSStatusBar 不持有 status item，必须自行保活（否则创建后立即从菜单栏消失）
    static STATUS_ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
    static BUTTON: RefCell<Option<Retained<NSStatusBarButton>>> = const { RefCell::new(None) };
    static MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
    /// 应用图标模板图，与文字合成到同一张内容图
    static ICON: RefCell<Option<Retained<NSImage>>> = const { RefCell::new(None) };
    static ENABLE_ITEM: RefCell<Option<Retained<NSMenuItem>>> = const { RefCell::new(None) };
    static TARGET: RefCell<Option<Retained<MenuTarget>>> = const { RefCell::new(None) };
}

define_class! {
    #[unsafe(super(NSObject))]
    struct MenuTarget;

    impl MenuTarget {
        /// 状态项点击与菜单项动作的统一入口。
        /// sender 可能是 NSStatusBarButton（点击状态项）或 NSMenuItem（菜单项）。
        #[unsafe(method(handleMenu:))]
        fn handle_menu(&self, sender: &NSObject) {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let Some(cb) = CALLBACKS.get() else { return };

            if sender.downcast_ref::<NSStatusBarButton>().is_some() {
                // 状态项点击：按鼠标键位分流
                let right = NSApplication::sharedApplication(mtm)
                    .currentEvent()
                    .map(|ev| ev.r#type() == NSEventType::RightMouseUp)
                    .unwrap_or(false);
                if right {
                    MENU.with_borrow(|menu| {
                        if let Some(menu) = menu {
                            let loc = NSEvent::mouseLocation();
                            menu.popUpMenuPositioningItem_atLocation_inView(None, loc, None);
                        }
                    });
                } else {
                    (cb.show_window)();
                }
                return;
            }

            if let Some(item) = sender.downcast_ref::<NSMenuItem>() {
                match item.tag() {
                    TAG_SHOW => (cb.show_window)(),
                    TAG_TOGGLE => {
                        let on = (cb.toggle_enabled)();
                        item.setState(if on {
                            NSControlStateValueOn
                        } else {
                            NSControlStateValueOff
                        });
                    }
                    TAG_UPDATE => (cb.check_update)(),
                    TAG_QUIT => (cb.quit)(),
                    _ => {}
                }
            }
        }
    }
}

/// 创建原生状态项（仅 macOS、仅主线程、应用生命周期内调用一次）
pub fn init(mtm: MainThreadMarker, callbacks: MenuCallbacks) -> Result<(), String> {
    let _ = CALLBACKS.set(callbacks);

    // 状态项（NSVariableStatusItemLength = -1.0：宽度随内容自适应）
    let bar = NSStatusBar::systemStatusBar();
    let item = bar.statusItemWithLength(-1.0);
    let button = item
        .button(mtm)
        .ok_or_else(|| "NSStatusItem 无 button".to_string())?;

    // 初始只显示图标（无数据时也有存在感），文字由 set_text 合成为图片
    let png = include_bytes!("../../icons/128x128.png");
    let data = unsafe { NSData::dataWithBytes_length(png.as_ptr().cast(), png.len()) };
    if let Some(img) = NSImage::initWithData(NSImage::alloc(), &data) {
        img.setSize(NSSize::new(MENU_ICON_SIZE, MENU_ICON_SIZE));
        img.setTemplate(true);
        button.setImage(Some(&img));
        ICON.with_borrow_mut(|slot| *slot = Some(img));
    }

    // 点击行为：左键开窗 / 右键弹菜单（sendActionOn 让两种按键都进 action，
    // 在 handle_menu 里按 currentEvent 分流）
    // NSObject::init 恒成功，Option 分支实际不会为 None
    let target: Retained<MenuTarget> = unsafe {
        msg_send![MenuTarget::alloc(), init]
    };
    unsafe {
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(handleMenu:)));
    }
    button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);

    // 菜单
    let menu = NSMenu::initWithTitle(
        NSMenu::alloc(mtm),
        &NSString::from_str("CC-Switch Analyzer"),
    );
    menu.setAutoenablesItems(false);

    let add_item = |title: &str, tag: isize| -> Retained<NSMenuItem> {
        let mi = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(sel!(handleMenu:)),
                &NSString::from_str(""),
            )
        };
        mi.setTag(tag);
        unsafe { mi.setTarget(Some(&target)) };
        mi.setEnabled(true);
        menu.addItem(&mi);
        mi
    };

    add_item("显示窗口", TAG_SHOW);
    menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    let toggle = add_item("启用菜单栏显示", TAG_TOGGLE);
    toggle.setState(NSControlStateValueOff);
    ENABLE_ITEM.with_borrow_mut(|slot| *slot = Some(toggle));
    menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    add_item("检查更新", TAG_UPDATE);
    add_item("退出", TAG_QUIT);

    // 保活并登记（STATUS_ITEM 必须保活：NSStatusBar 不持有它）
    STATUS_ITEM.with_borrow_mut(|slot| *slot = Some(item));
    BUTTON.with_borrow_mut(|slot| *slot = Some(button));
    MENU.with_borrow_mut(|slot| *slot = Some(menu));
    TARGET.with_borrow_mut(|slot| *slot = Some(target));

    log::info!("[Menubar] 原生 NSStatusItem 已创建");
    Ok(())
}

/// 把「图标 + 两行小字」渲染成一张模板图，作为状态项的唯一内容。
///
/// 为什么不用 `title`：实测 `NSStatusBarButtonCell` 对多行标题是**顶对齐**
/// 而非居中（macOS 27 下按钮 24pt 高，文字块中心被排到 30pt，整块偏上
/// 18pt；缩小字号只会让底部空得更多，顶边纹丝不动）。`attributedTitle` 的
/// `baselineOffset` 同样被 cell 忽略（试过，文字纹丝不动），只有 `image`
/// 会被精确居中（`imageRect` 实测上下留白对称）。因此改为位图化：排版
/// 完全自己控制，交给 AppKit 居中，深浅色由模板图自动适配。
fn render_content_image(mtm: MainThreadMarker, tokens: &str) -> Option<Retained<NSImage>> {
    let icon = ICON.with_borrow(|slot| slot.clone())?;

    // 文字富文本：9pt 等宽数字、左对齐（配合前导空格实现数字右对齐）、无段落间距
    let font = NSFont::monospacedDigitSystemFontOfSize_weight(MENU_FONT_SIZE, 0.0);
    // NSObject::init 恒成功
    let para: Retained<NSMutableParagraphStyle> = unsafe {
        msg_send![NSMutableParagraphStyle::alloc(), init]
    };
    // 左对齐：前导空格是「数字右对齐」的实现手段，居中会吞掉行首空格
    para.setAlignment(NSTextAlignment::Left);
    para.setParagraphSpacing(0.0);
    // 模板图只看 alpha 通道，黑色即可（深浅色由系统渲染时决定）
    let color = NSColor::blackColor();
    let attributed = unsafe {
        let font_any: &AnyObject = &*(Retained::as_ptr(&font) as *const AnyObject);
        let para_any: &AnyObject = &*(Retained::as_ptr(&para) as *const AnyObject);
        let color_any: &AnyObject = &*(Retained::as_ptr(&color) as *const AnyObject);
        let dict = NSDictionary::from_slices(
            &[
                NSFontAttributeName,
                NSParagraphStyleAttributeName,
                NSForegroundColorAttributeName,
            ],
            &[font_any, para_any, color_any],
        );
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(tokens),
            Some(&dict),
        )
    };
    // 文本测量与绘制走 CoreText：objc2-app-kit 0.3.2 未为
    // NSAttributedString 生成 AppKit 分类方法（size/drawWithRect:），
    // 经 msg_send! 调用时结构体传参与返回值在本机 arm64 上不可靠
    // （实测 size 返回 8.03×12 垃圾值、drawWithRect: 直接抛异常 abort）
    let framesetter = unsafe { CTFramesetter::with_attributed_string(attributed.as_ref()) };
    // CFRange.length 是 UTF-16 码元数，不能用 UTF-8 字节数
    let full_range = CFRange {
        location: 0,
        length: attributed.length() as isize,
    };
    let text_size: NSSize = unsafe {
        framesetter.suggest_frame_size_with_constraints(
            full_range,
            None,
            NSSize::new(f64::MAX, f64::MAX),
            std::ptr::null_mut(),
        )
    };

    // 无文字时只画图标
    let has_text = !tokens.is_empty();
    let width = if has_text {
        MENU_ICON_SIZE + MENU_ICON_GAP + text_size.width
    } else {
        MENU_ICON_SIZE
    };
    let height = if has_text {
        text_size.height.max(MENU_ICON_SIZE)
    } else {
        MENU_ICON_SIZE
    };
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    // 2x 位图
    let px_w = (width * MENU_IMAGE_SCALE).ceil() as usize;
    let px_h = (height * MENU_IMAGE_SCALE).ceil() as usize;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            px_w as isize,
            px_h as isize,
            8,
            4,
            true,
            false,
            &NSString::from_str("NSCalibratedRGBColorSpace"),
            0,
            0,
        )
    }?;
    // 位图像素是 2x，把逻辑尺寸设为点尺寸，绘制坐标即以 pt 为单位
    rep.setSize(NSSize::new(width, height));
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;

    // 直接在位图默认（不翻转）坐标系里绘制：CoreText 的 CTFrameDraw
    // 会自行处理上下文方向，额外翻转 CTM 反而会把文字上下颠倒
    unsafe {
        NSGraphicsContext::setCurrentContext(Some(&ctx));
        ctx.saveGraphicsState();

        if has_text {
            // 文字与图标在合成图内都按自身高度垂直居中
            let text_y = (height - text_size.height) / 2.0;
            let path = CGPath::with_rect(
                CGRect::new(
                    NSPoint::new(MENU_ICON_SIZE + MENU_ICON_GAP, text_y),
                    NSSize::new(text_size.width, text_size.height),
                ),
                std::ptr::null(),
            );
            let frame = framesetter.frame(full_range, &path, None);
            CTFrame::draw(&frame, &ctx.CGContext());
        }

        // 图标：模板图，绘制时按 mask 填充，画到透明位图上即正确的 alpha
        let icon_rect = NSRect::new(
            NSPoint::new(0.0, (height - MENU_ICON_SIZE) / 2.0),
            NSSize::new(MENU_ICON_SIZE, MENU_ICON_SIZE),
        );
        let _: () = msg_send![
            &icon,
            drawInRect: icon_rect,
            fromRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            operation: NSCompositingOperation::SourceOver,
            fraction: 1.0
        ];

        ctx.restoreGraphicsState();
        NSGraphicsContext::setCurrentContext(None);
    }

    let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(width, height));
    image.addRepresentation(&rep);
    image.setTemplate(true);
    Some(image)
}

/// 拆出「数值部分」与「单位后缀」：`345.60k` → `("345.60", "k")`，
/// `33.98¥` → `("33.98", "¥")`。切分点是首个非数字非小数点的字符（按字节，
/// 因单位恒为 ASCII 或 `¥`）。
fn split_value_unit(s: &str) -> (&str, &str) {
    match s.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(i) => s.split_at(i),
        None => (s, ""),
    }
}

/// 上下两行排版：数字右对齐、单位落在同一列。
///
/// 两行的整数位数可能不同（`345.60k` vs `33.98¥`），等宽数字字体下以
/// 较短一行的数值部分补前导空格到同一宽度即可对齐；单位跟在数值之后，
/// 自然同列。配合渲染层的左对齐段落（前导空格才不会被吞），得到整齐的
/// 「数字列 + 单位列」两行。
fn align_two_lines(tokens: &str, cost: &str) -> String {
    let (t_num, t_unit) = split_value_unit(tokens);
    let (c_num, c_unit) = split_value_unit(cost);
    let width = t_num
        .chars()
        .count()
        .max(c_num.chars().count())
        .max(1);
    let pad = |num: &str| {
        let pad_len = width - num.chars().count();
        format!("{}{num}", " ".repeat(pad_len))
    };
    format!(
        "{}{t_unit}\n{}{c_unit}",
        pad(t_num),
        pad(c_num)
    )
}

/// 更新状态项内容：图标 + 两行小字（上行 token / 下行费用）合成模板图。
/// `tokens` 为空字符串时只保留图标。
pub fn set_text(mtm: MainThreadMarker, tokens: String, cost: String) {
    // 费用缺失时只渲染单行，避免尾部空行撑高文字块
    let text = match (tokens.is_empty(), cost.is_empty()) {
        (true, _) => String::new(),
        (false, true) => tokens,
        _ => align_two_lines(&tokens, &cost),
    };

    BUTTON.with_borrow(|slot| {
        let Some(button) = slot else { return };
        let Some(image) = render_content_image(mtm, &text) else {
            return;
        };
        // 内容只有一张合成图、无 title，用 ImageOnly 让 cell 整体居中绘制它
        button.setImagePosition(NSCellImagePosition::ImageOnly);
        button.setImage(Some(&image));

        if !GEOMETRY_LOGGED.swap(true, Ordering::Relaxed) {
            let bounds = button.bounds();
            let image_rect = button
                .cell()
                .map(|c| c.imageRectForBounds(bounds))
                .unwrap_or_default();
            log::info!(
                "[Menubar] 几何: button.frame={:?} bounds.midY={:.1} imageRect={:?}",
                button.frame(),
                bounds.origin.y + bounds.size.height / 2.0,
                image_rect,
            );
        }
    });
}

/// 同步"启用菜单栏显示"菜单项勾选态
pub fn set_toggle_state(on: bool) {
    ENABLE_ITEM.with_borrow(|slot| {
        if let Some(item) = slot {
            item.setState(if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_value_unit() {
        assert_eq!(split_value_unit("345.60k"), ("345.60", "k"));
        assert_eq!(split_value_unit("33.98¥"), ("33.98", "¥"));
        assert_eq!(split_value_unit("999.00"), ("999.00", ""));
    }

    #[test]
    fn test_align_two_lines() {
        // 整数位数相同：无需补空格，单位自然同列
        assert_eq!(align_two_lines("84.30M", "33.98¥"), "84.30M\n33.98¥");
        // 整数位数不同：短行数值前补空格，数字右对齐、单位仍在同一列
        assert_eq!(align_two_lines("345.60k", "33.98¥"), "345.60k\n 33.98¥");
        // 费用 4 位整数时 Token 行补 2 个空格
        assert_eq!(align_two_lines("84.30M", "1725.50¥"), "  84.30M\n1725.50¥");
    }
}
