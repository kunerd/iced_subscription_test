use std::any::TypeId;
use std::collections::HashMap;
use std::time::Duration;

use iced::futures::channel::mpsc::Sender;
use iced::futures::never::Never;
use iced::futures::stream::{unfold, SelectAll};
use iced::futures::{FutureExt, SinkExt, StreamExt};
use iced::{subscription, widget, Application, Command, Element, Settings, Subscription};
use rand::Rng;
use tokio::sync::mpsc;

fn main() -> iced::Result {
    App::run(Settings::default())
}

struct App {
    id_counter: usize,
    state: AppState,
}

enum AppState {
    Init,
    Running {
        downloader: Downloader<usize>,
        downloads: HashMap<usize, Download>,
    },
}

struct Download {
    url: String,
    progress: f32,
}

#[derive(Debug, Clone)]
enum Message {
    DownloaderEvent(DownloaderEvent<usize>),
    StartDownload,
    Clear,
}

impl Application for App {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = iced::theme::Theme;
    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let app = App {
            id_counter: 0,
            state: AppState::Init,
        };
        (app, Command::none())
    }

    fn title(&self) -> String {
        "Subscription_Test".to_string()
    }

    fn update(&mut self, message: Self::Message) -> iced::Command<Self::Message> {
        match message {
            Message::DownloaderEvent(DownloaderEvent::Initialized(d)) => {
                self.state = AppState::Running {
                    downloader: d,
                    downloads: HashMap::new(),
                };

                Command::none()
            }
            Message::DownloaderEvent(DownloaderEvent::Progress(id, p)) => {
                let AppState::Running { downloads, .. } = &mut self.state else {
                    return Command::none();
                };

                let Progress::Advanced(p) = p else {
                    return Command::none();
                };

                if let Some(download) = downloads.get_mut(&id) {
                    download.progress = p
                }

                Command::none()
            }
            Message::StartDownload => {
                let AppState::Running {
                    downloader,
                    downloads,
                } = &mut self.state
                else {
                    return Command::none();
                };

                let url = format!("http://somer.server/files/{}", self.id_counter);
                downloads.insert(
                    self.id_counter,
                    Download {
                        url: url.clone(),
                        progress: 0.0,
                    },
                );
                downloader.download(self.id_counter, url.clone());
                self.id_counter += 1;

                Command::none()
            }
            Message::Clear => {
                let AppState::Running { downloads, .. } = &mut self.state else {
                    return Command::none();
                };

                downloads.clear();

                Command::none()
            }
        }
    }

    fn view(&self) -> iced::Element<'_, Self::Message, Self::Theme, iced::Renderer> {
        match &self.state {
            AppState::Init => widget::text("App is initializing...").into(),
            AppState::Running { downloads, .. } => {
                let downloads: Vec<Element<_>> = downloads
                    .values()
                    .map(|d| {
                        widget::column!(
                            widget::text(d.url.clone()),
                            widget::progress_bar(0f32..=100f32, d.progress)
                        )
                        .into()
                    })
                    .collect();

                let downloads = widget::column(downloads).padding(10);
                let download_btn =
                    widget::button("Start Download").on_press(Message::StartDownload);
                let clear_btn = widget::button("Clear").on_press(Message::Clear);

                widget::column!(widget::row!(download_btn, clear_btn), downloads).into()
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        download_worker().map(Message::DownloaderEvent)
    }
}

fn download_worker<I: Copy + Send + 'static>() -> Subscription<DownloaderEvent<I>> {
    let id = TypeId::of::<Downloader<I>>();

    subscription::channel(id, 128, run)
}

async fn run<I: Copy>(mut sender: Sender<DownloaderEvent<I>>) -> Never {
    let (tx, mut rx) = mpsc::channel(32);
    let downloader = Downloader { sender: tx };

    let _ = sender.send(DownloaderEvent::Initialized(downloader)).await;

    let mut downloads = SelectAll::new();
    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                match msg {
                    DownloaderMessage::Download(id, url) => downloads
                        .push(unfold(State::Ready(url), move |state| {
                            Box::pin(download(id, state).map(Some))
                        })),
                }
            }
            Some((i, p)) = downloads.next() => {
                let _ = sender.send(DownloaderEvent::Progress(i, p)).await;
            }
        }
    }
}

#[derive(Debug, Clone)]
enum DownloaderEvent<I> {
    Initialized(Downloader<I>),
    Progress(I, Progress),
}

#[derive(Debug, Clone)]
struct Downloader<I> {
    sender: mpsc::Sender<DownloaderMessage<I>>,
}

impl<I> Downloader<I> {
    fn download(&self, id: I, url: String) {
        let _ = self.sender.try_send(DownloaderMessage::Download(id, url));
    }
}

enum DownloaderMessage<I> {
    Download(I, String),
}

async fn download<I: Copy>(id: I, state: State) -> ((I, Progress), State) {
    match state {
        State::Ready(_url) => {
            let mut rng = rand::thread_rng();
            let total = rng.gen_range(10_000..50_000);
            (
                (id, Progress::Started),
                State::Downloading {
                    total,
                    downloaded: 0,
                },
            )
        }
        State::Downloading { total, downloaded } => {
            if downloaded <= total {
                let (chunk_size, sleep) = {
                    let mut rng = rand::thread_rng();
                    (rng.gen_range(1_000..5_000), rng.gen_range(100..500))
                };

                tokio::time::sleep(Duration::from_millis(sleep)).await;

                let downloaded = downloaded + chunk_size;
                let percentage = (downloaded as f32 / total as f32) * 100.0;

                (
                    (id, Progress::Advanced(percentage)),
                    State::Downloading { total, downloaded },
                )
            } else {
                ((id, Progress::Finished), State::Finished)
            }
        }
        State::Finished => iced::futures::future::pending().await,
    }
}

#[derive(Debug, Clone)]
pub enum Progress {
    Started,
    Advanced(f32),
    Finished,
}

pub enum State {
    Ready(String),
    Downloading { total: u64, downloaded: u64 },
    Finished,
}
