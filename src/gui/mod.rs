//! The portboard window.

mod board;
mod theme;
mod widgets;

use std::time::Duration;

use gpui_kit::*;

use crate::manager::Manager;
use board::Board;

pub fn run(manager: Manager) {
    application().with_assets(gpui_kit::assets::AllAssets).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        theme::install(cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(900.), px(560.))),
            titlebar: Some(TitlebarOptions { title: Some("portboard".into()), ..Default::default() }),
            app_id: Some("portboard".into()),
            ..Default::default()
        };
        let Ok((_, view)) = gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| Board::new(manager, window, cx)))
        else {
            eprintln!("portboard: couldn't open a window");
            cx.quit();
            return;
        };

        // Closing the window quits; stop every server on the way out.
        let on_quit = view.clone();
        cx.on_app_quit(move |cx| {
            on_quit.update(cx, |board, _| board.shutdown());
            async {}
        })
        .detach();

        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let quit = crate::terminated();
                cx.update(|cx| {
                    if quit {
                        cx.quit();
                    } else {
                        view.update(cx, |board, cx| board.tick(cx));
                    }
                });
                if quit {
                    break;
                }
            }
        })
        .detach();
    });
}
