//! Local backup snapshot reconciliation and disaster recovery panel.

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use release_desk_core::snapshot::MismatchReport;

pub fn render_reconcile<T: 'static>(
    report_opt: Option<&MismatchReport>,
    cx: &Context<T>,
    on_restore_db: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_sync_backup: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let mut panel = div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .overflow_y_scrollbar()
        .size_full()
        .child(
            div()
                .font_bold()
                .text_size(px(24.))
                .child("Local Backup & Database Reconciliation"),
        )
        .child(
            "Every project clone location automatically maintains a local .release-desk/data.json \
             backup snapshot. If the database is lost or diverges, you can restore or synchronize state here.",
        );

    let (status_title, status_desc, is_synced) = match report_opt {
        Some(report) if report.is_synchronized() => (
            "✓ SYNCHRONIZED",
            "SQLite database and local data.json backup snapshot match perfectly.",
            true,
        ),
        Some(_) => (
            "⚠ MISMATCH DETECTED",
            "Discrepancies found between database and local data.json backup snapshot.",
            false,
        ),
        None => (
            "ℹ CHECKING STATUS",
            "Inspect or select a project with a clone location.",
            false,
        ),
    };

    let status_card = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .rounded_lg()
        .bg(if is_synced {
            cx.theme().accent
        } else {
            cx.theme().danger
        })
        .text_color(cx.theme().accent_foreground)
        .child(div().font_bold().text_size(px(20.)).child(status_title))
        .child(status_desc);

    panel = panel.child(status_card);

    if let Some(report) = report_opt {
        let missing_db_str = if report.missing_in_database.is_empty() {
            "None (0)".to_string()
        } else {
            format!("{:?}", report.missing_in_database)
        };
        let missing_disk_str = if report.missing_on_disk.is_empty() {
            "None (0)".to_string()
        } else {
            format!("{:?}", report.missing_on_disk)
        };

        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .child(format!("Project Root: {}", report.project_root.display()))
                .child(format!(
                    "Matched Repositories: {}",
                    report.matched_repositories.len()
                ))
                .child(format!(
                    "Missing in Database (Backup Only): {}",
                    missing_db_str
                ))
                .child(format!("Missing on Disk (DB Only): {}", missing_disk_str))
                .child(format!(
                    "Revision Divergences: {}",
                    report.revision_mismatches.len()
                )),
        );
    }

    panel = panel.child(
        div()
            .flex()
            .gap_3()
            .child(
                Button::new("restore-db")
                    .primary()
                    .label("Restore Database from Backup")
                    .on_click(cx.listener(move |this, _, _, cx| on_restore_db(this, cx))),
            )
            .child(
                Button::new("sync-backup")
                    .label("Export Database to Backup")
                    .on_click(cx.listener(move |this, _, _, cx| on_sync_backup(this, cx))),
            ),
    );

    panel
}
