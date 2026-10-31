use crate::actions::Action;

#[cfg(test)]
#[path = "tray_tests.rs"]
mod tests;
use eframe::egui;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    time::Duration,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Str, Structure};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/Menu";
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;
type Properties = HashMap<String, OwnedValue>;
type Layout = (i32, Properties, Vec<OwnedValue>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Starting,
    Ready,
    Unavailable,
}

struct Shared {
    status: AtomicU8,
    hidden: AtomicBool,
    actions: mpsc::SyncSender<Action>,
    ctx: egui::Context,
}
impl Shared {
    fn send(&self, action: Action) -> zbus::fdo::Result<()> {
        self.actions
            .try_send(action)
            .map_err(|_| zbus::fdo::Error::LimitsExceeded("The player is busy".into()))?;
        self.ctx.request_repaint();
        Ok(())
    }
    fn show(&self) {
        self.hidden.store(false, Ordering::Release);
        self.ctx
            .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Minimized(false),
        );
        self.ctx.request_repaint();
    }
    fn status(&self, value: u8) {
        let old = self.status.swap(value, Ordering::AcqRel);
        if value != 1 && self.hidden.load(Ordering::Acquire) {
            self.show();
        }
        if value != old {
            self.ctx.request_repaint();
        }
    }
}
struct RestoreOnExit(Arc<Shared>);
impl Drop for RestoreOnExit {
    fn drop(&mut self) {
        self.0.status(2);
    }
}

pub struct Tray {
    shared: Arc<Shared>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    pub events: mpsc::Receiver<Action>,
}
impl Tray {
    pub fn new(ctx: egui::Context) -> Self {
        let (actions, events) = mpsc::sync_channel(32);
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let shared = Arc::new(Shared {
            status: AtomicU8::new(0),
            hidden: AtomicBool::new(false),
            actions,
            ctx,
        });
        let worker = shared.clone();
        let started = std::thread::Builder::new()
            .name("tidal-tray".into())
            .spawn(move || {
                let _restore = RestoreOnExit(worker.clone());
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(async move {
                        tokio::select! {
                            _ = &mut stopped => {},
                            _ = serve(worker) => {},
                        }
                    });
                }
            });
        if started.is_err() {
            shared.status(2);
        }
        Self {
            shared,
            stop: Some(stop),
            events,
        }
    }
    pub fn status(&self) -> Status {
        match self.shared.status.load(Ordering::Acquire) {
            0 => Status::Starting,
            1 => Status::Ready,
            _ => Status::Unavailable,
        }
    }
    pub fn hide(&self) -> bool {
        if self.status() != Status::Ready {
            return false;
        }
        self.shared.hidden.store(true, Ordering::Release);
        self.shared.ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Visible(false),
        );
        // Recheck readiness after publishing the hidden state so host loss cannot strand us.
        if self.status() != Status::Ready || !self.shared.hidden.load(Ordering::Acquire) {
            self.shared.show();
            return false;
        }
        true
    }
    pub fn show(&self) {
        self.shared.show();
    }
    #[cfg(test)]
    pub fn fixture(ctx: egui::Context, ready: bool) -> Self {
        let (actions, events) = mpsc::sync_channel(32);
        Self {
            shared: Arc::new(Shared {
                status: AtomicU8::new(if ready { 1 } else { 2 }),
                hidden: AtomicBool::new(false),
                actions,
                ctx,
            }),
            stop: None,
            events,
        }
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        if self.shared.hidden.load(Ordering::Acquire) {
            self.shared.show();
        }
        self.stop.take();
    }
}

fn icon() -> Pixmaps {
    let Ok(image) = image::load_from_memory(include_bytes!("../assets/icon.png")) else {
        return Vec::new();
    };
    let image = image
        .resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let bytes = image
        .pixels()
        .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
        .collect();
    vec![(64, 64, bytes)]
}

async fn serve(shared: Arc<Shared>) -> anyhow::Result<()> {
    let connection = tokio::time::timeout(
        Duration::from_secs(3),
        zbus::ConnectionBuilder::session()?
            .max_queued(32)
            .serve_at(ITEM_PATH, Item(shared.clone(), icon()))?
            .serve_at(MENU_PATH, Menu(shared.clone()))?
            .build(),
    )
    .await??;
    let mut registered = None;
    loop {
        let ready = tokio::time::timeout(
            Duration::from_secs(1),
            register(&connection, &mut registered),
        )
        .await;
        shared.status(if matches!(ready, Ok(Ok(true))) { 1 } else { 2 });
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn register(
    connection: &zbus::Connection,
    registered: &mut Option<String>,
) -> anyhow::Result<bool> {
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    let owner = bus.get_name_owner(WATCHER.try_into()?).await?.to_string();
    let proxy: zbus::Proxy<'_> = zbus::ProxyBuilder::new_bare(connection)
        .destination(owner.as_str())?
        .path(WATCHER_PATH)?
        .interface(WATCHER)?
        .cache_properties(zbus::CacheProperties::No)
        .build()
        .await?;
    let unique = connection
        .unique_name()
        .ok_or_else(|| anyhow::anyhow!("No session bus identity"))?
        .as_str();
    let identity = format!("{unique}{ITEM_PATH}");
    let items: Vec<String> = proxy.get_property("RegisteredStatusNotifierItems").await?;
    let present = |items: &[String]| items.iter().any(|item| item == &identity || item == unique);
    if registered.as_deref() != Some(owner.as_str()) || !present(&items) {
        proxy
            .call::<_, _, ()>("RegisterStatusNotifierItem", &(ITEM_PATH,))
            .await?;
        *registered = Some(owner.clone());
    }
    let items: Vec<String> = proxy.get_property("RegisteredStatusNotifierItems").await?;
    Ok(present(&items)
        && proxy
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .await?)
}

struct Item(Arc<Shared>, Pixmaps);
#[zbus::dbus_interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[dbus_interface(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }
    #[dbus_interface(property)]
    fn id(&self) -> &str {
        "tidal-forces"
    }
    #[dbus_interface(property)]
    fn title(&self) -> &str {
        "Tidal Forces"
    }
    #[dbus_interface(property)]
    fn status(&self) -> &str {
        "Active"
    }
    #[dbus_interface(property)]
    fn window_id(&self) -> i32 {
        0
    }
    #[dbus_interface(property)]
    fn icon_theme_path(&self) -> &str {
        ""
    }
    #[dbus_interface(property)]
    fn icon_name(&self) -> &str {
        "rocks.tidalforces.Player"
    }
    #[dbus_interface(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        self.1.clone()
    }
    #[dbus_interface(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }
    #[dbus_interface(property)]
    fn overlay_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[dbus_interface(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }
    #[dbus_interface(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[dbus_interface(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }
    #[dbus_interface(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[dbus_interface(property)]
    fn menu(&self) -> OwnedObjectPath {
        MENU_PATH.try_into().unwrap()
    }
    #[dbus_interface(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        (
            "rocks.tidalforces.Player".into(),
            Vec::new(),
            "Tidal Forces".into(),
            "Open the player or use the playback menu".into(),
        )
    }
    fn activate(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        self.0.send(Action::Raise)
    }
    fn secondary_activate(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        self.0.send(Action::Toggle)
    }
    fn context_menu(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        self.0.send(Action::Raise)
    }
    fn provide_xdg_activation_token(&self, token: &str) -> zbus::fdo::Result<()> {
        if token.len() > 4096 {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Invalid activation token".into(),
            ));
        }
        Ok(())
    }
    fn scroll(&self, delta: i32, orientation: &str) -> zbus::fdo::Result<()> {
        if orientation == "vertical" {
            self.0.send(Action::VolumeBy(
                (delta as f32 / 120. * 0.02).clamp(-0.1, 0.1),
            ))
        } else {
            Ok(())
        }
    }
}

const LABELS: [&str; 8] = [
    "Tidal Forces",
    "Open player",
    "Now Playing",
    "Mini player",
    "Play / pause",
    "Previous",
    "Next",
    "Quit",
];
fn properties(id: i32, names: &[String]) -> zbus::fdo::Result<Properties> {
    let label = LABELS
        .get(usize::try_from(id).unwrap_or(usize::MAX))
        .ok_or_else(|| zbus::fdo::Error::InvalidArgs("Unknown menu item".into()))?;
    if names.len() > 32 || names.iter().any(|name| name.len() > 64) {
        return Err(zbus::fdo::Error::LimitsExceeded(
            "Too many menu properties".into(),
        ));
    }
    let mut values = Properties::from([
        ("label".into(), OwnedValue::from(Str::from(*label))),
        ("enabled".into(), OwnedValue::from(true)),
        ("visible".into(), OwnedValue::from(true)),
    ]);
    if id == 0 {
        values.insert(
            "children-display".into(),
            OwnedValue::from(Str::from("submenu")),
        );
    }
    if !names.is_empty() {
        values.retain(|name, _| names.contains(name));
    }
    Ok(values)
}
fn menu_action(id: i32) -> Option<Action> {
    Some(match id {
        1 => Action::Raise,
        2 => Action::NowPlaying,
        3 => Action::MiniPlayer,
        4 => Action::Toggle,
        5 => Action::Previous,
        6 => Action::Next,
        7 => Action::Quit,
        _ => return None,
    })
}

struct Menu(Arc<Shared>);
#[zbus::dbus_interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[dbus_interface(property)]
    fn version(&self) -> u32 {
        4
    }
    #[dbus_interface(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }
    #[dbus_interface(property)]
    fn status(&self) -> &str {
        "normal"
    }
    #[dbus_interface(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        if recursion_depth < -1 {
            return Err(zbus::fdo::Error::InvalidArgs("Invalid menu depth".into()));
        }
        let props = properties(parent_id, &property_names)?;
        let children = if parent_id == 0 && recursion_depth != 0 {
            (1..LABELS.len() as i32)
                .map(|id| {
                    Ok(OwnedValue::from(Structure::from((
                        id,
                        properties(id, &property_names)?,
                        Vec::<OwnedValue>::new(),
                    ))))
                })
                .collect::<zbus::fdo::Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        Ok((1, (parent_id, props, children)))
    }
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<Vec<(i32, Properties)>> {
        if ids.len() > 64 {
            return Err(zbus::fdo::Error::LimitsExceeded(
                "Too many menu items".into(),
            ));
        }
        let ids = if ids.is_empty() {
            (0..LABELS.len() as i32).collect()
        } else {
            ids
        };
        ids.into_iter()
            .filter(|id| *id >= 0 && (*id as usize) < LABELS.len())
            .map(|id| Ok((id, properties(id, &property_names)?)))
            .collect()
    }
    fn get_property(&self, id: i32, name: &str) -> zbus::fdo::Result<OwnedValue> {
        properties(id, &[])?
            .remove(name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("Unknown menu property".into()))
    }
    fn event(
        &self,
        id: i32,
        event_id: &str,
        _data: OwnedValue,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        properties(id, &[])?;
        if event_id == "clicked" {
            self.0.send(menu_action(id).ok_or_else(|| {
                zbus::fdo::Error::InvalidArgs("Menu item has no action".into())
            })?)?;
        }
        Ok(())
    }
    fn event_group(
        &self,
        events: Vec<(i32, String, OwnedValue, u32)>,
    ) -> zbus::fdo::Result<Vec<i32>> {
        if events.len() > 32 {
            return Err(zbus::fdo::Error::LimitsExceeded(
                "Too many menu events".into(),
            ));
        }
        Ok(events
            .into_iter()
            .filter_map(|(id, event, data, timestamp)| {
                self.event(id, &event, data, timestamp).err().map(|_| id)
            })
            .collect())
    }
    fn about_to_show(&self, id: i32) -> zbus::fdo::Result<bool> {
        properties(id, &[])?;
        Ok(false)
    }
    fn about_to_show_group(&self, ids: Vec<i32>) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        if ids.len() > 64 {
            return Err(zbus::fdo::Error::LimitsExceeded(
                "Too many menu items".into(),
            ));
        }
        Ok((
            Vec::new(),
            ids.into_iter()
                .filter(|id| properties(*id, &[]).is_err())
                .collect(),
        ))
    }
}
