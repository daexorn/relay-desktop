use super::*;
use crate::cluster::{Agent, Config, Run, TaskState};
use crate::templates::{self, Context, Snippet, Variable};

pub(super) struct Editor {
    config: Config,
    is_new: bool,
    tab: usize,
    picked: usize,
    error: String,
    snippets: Vec<Snippet>,
    search: String,
    preview: bool,
}

pub(super) struct ChatOrchestraDialog {
    pub(super) server_id: String,
    pub(super) chat_key: String,
    pub(super) sid: String,
    pub(super) palette: crate::cluster::ModelPalette,
    pub(super) max_parallel: usize,
    pub(super) auto_results: bool,
    pub(super) active_run_id: Option<String>,
    pub(super) has_existing_workers: bool,
    pub(super) update_existing: bool,
    pub(super) show_tiers: bool,
}
impl RelayApp {
    pub(super) fn seed_orchestration_preview(&mut self, ctx: &egui::Context, args: &[String]) {
        let preview = args
            .iter()
            .any(|a| a.starts_with("--demo-cluster-") || a == "--demo-leader");
        if preview {
            self.seed_cluster_demo();
            self.settings.prompt_snippets=vec![Snippet{key:"контекст_проекта".into(),value:"Проект Relay. Исходники: [данные_папки_(Production)]. Сначала изучи существующие правила проекта.".into()}];
            let cfg = &mut self.cluster_store.runs.last_mut().unwrap().config;
            cfg.shared_prompt = "Команда: [имя_кластера].\n[контекст_проекта]".into();
            cfg.leader.prompt="Ты руководитель команды. Распредели задачи и проверь результаты.\nОсновной сервер: [данные_айпи_(Production)]\nРабочая папка: [рабочая_папка]\nВ конце перечисли реальные проверки.".into();
            let cfg = cfg.clone();
            self.settings.orchestrators = vec![cfg.clone()];
            if args.iter().any(|a| {
                a == "--demo-cluster-editor"
                    || a == "--demo-cluster-prompts"
                    || a == "--demo-cluster-keys"
            }) {
                self.cluster_open = true;
                self.edit_cluster_config(cfg);
                let e = self.cluster_edit.as_mut().unwrap();
                if args.iter().any(|a| a == "--demo-cluster-prompts") {
                    e.tab = 1;
                    e.picked = 1;
                    e.preview = true;
                }
                if args.iter().any(|a| a == "--demo-cluster-keys") {
                    e.tab = 3;
                }
            }
            if args.iter().any(|a| a == "--demo-leader") {
                let r = self.cluster_store.runs.last().unwrap();
                let sid = r.leader_sid.clone();
                let server = r.config.leader.server.clone();
                let mut c = Chat::new(
                    sid.clone(),
                    "/opt/atlas".into(),
                    model::shorten(&r.title, 70),
                );
                c.loaded = true;
                let result = r.task_json(&r.tasks[1].id, true).unwrap();
                c.messages=vec![Message{role:"user".into(),text:r.title.clone(),..Default::default()},Message{role:"assistant".into(),text:"Передал работу над интерфейсом исполнителю. Проверка совместимости уже завершена; жду второй результат и затем сверю сборку.".into(),..Default::default()},Message{role:"user".into(),text:format!("<relay-cluster-results>\n[{}]\n</relay-cluster-results>",result),..Default::default()}];
                self.chats.insert(key(&server, &sid), c);
                self.settings.selected = Some(server.clone());
                self.selected_chat.insert(server.clone(), sid.clone());
                if let Some(remote) = self.remote.get_mut(&server) {
                    remote.sessions.insert(
                        0,
                        Session {
                            id: sid,
                            title: "Команда Relay".into(),
                            cwd: "/opt/atlas".into(),
                            modified: crate::cluster::now(),
                            ..Default::default()
                        },
                    );
                }
                self.cluster_open = false;
            }
        }
        if args.iter().any(|a| a == "--demo-command-output") {
            if let Some(k) = self.current_key() {
                let c = self.chats.get_mut(&k).unwrap();
                c.messages=vec![Message{role:"assistant".into(),text:"Проверяю сборку и тесты.".into(),..Default::default()},
                    Message{role:"tool".into(),tool:"Bash".into(),tool_id:"build".into(),input:json!({"command":"cargo build --release"}),..Default::default()},
                    Message{role:"tool_result".into(),tool_id:"build".into(),text:"Compiling relay-desktop v0.7.0\nFinished release profile in 42.3s".into(),..Default::default()},
                    Message{role:"tool".into(),tool:"mcp__relay_desktop__run_local_command".into(),tool_id:"test".into(),input:json!({"command":"cargo test --locked"}),..Default::default()},
                    Message{role:"tool_result".into(),tool_id:"test".into(),text:json!({"ok":true,"exit_code":0,"stdout":"running tests\nAll tests passed\nNo failures or ignored checks.","stderr":""}).to_string(),..Default::default()},
                    Message{role:"assistant".into(),text:"Сборка и проверки завершены. Результаты приведены в карточках команд.".into(),..Default::default()}];
                let id = egui::Id::new(("activity-group", &k, 1usize));
                for id in [id, id.with(0usize), id.with(1usize)] {
                    let mut state =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ctx, id, true,
                        );
                    state.set_open(true);
                    state.store(ctx);
                }
            }
        }
    }
    pub(super) fn default_cluster_config(&self) -> Config {
        let active_backend = self
            .server()
            .or_else(|| self.settings.servers.first().cloned())
            .map(|s| s.effective_backend().to_owned())
            .unwrap_or_else(|| "claude".into());
        if active_backend == "agy" {
            self.preset_cluster_config("agy")
        } else {
            self.preset_cluster_config("classic")
        }
    }
    pub(super) fn preset_cluster_config(&self, kind: &str) -> Config {
        let servers = &self.settings.servers;
        let mut c = Config::default();
        let Some(s) = self.server().or_else(|| servers.first().cloned()) else {
            return c;
        };
        match kind {
            "agy" => {
                c.name = "Оркестр AGY".into();
                c.max_parallel = 2;
                c.mode = "auto".into();
                c.leader = Agent {
                    name: "Архитектор (AGY)".into(),
                    server: s.id.clone(),
                    backend: "agy".into(),
                    model: "inherit".into(),
                    effort: "default".into(),
                    mode: String::new(),
                    prompt: "Ты руководитель оркестра. Распределяй подзадачи через relay_cluster (list_workers -> dispatch_task -> wait_for_task), своди результаты и проверяй качество.".into(),
                    ..Default::default()
                };
                c.workers = vec![
                    Agent {
                        name: "Разработчик".into(),
                        server: s.id.clone(),
                        backend: "agy".into(),
                        model: "inherit".into(),
                        effort: "default".into(),
                        mode: "auto".into(),
                        prompt: "Реализуй назначенную задачу, соблюдай стиль проекта, не ломай чужие правки и отчитайся о файлах и проверках.".into(),
                        ..Default::default()
                    },
                    Agent {
                        name: "Ревьюер".into(),
                        server: s.id.clone(),
                        backend: "agy".into(),
                        model: "inherit".into(),
                        effort: "default".into(),
                        mode: "plan".into(),
                        prompt: "Проверь изменения, найди баги, регрессии и пробелы в тестах. Не вноси правки без явного указания руководителя.".into(),
                        ..Default::default()
                    },
                ];
            }
            "mixed" => {
                c.name = "Смешанный оркестр".into();
                c.max_parallel = 2;
                c.mode = "auto".into();
                c.leader = Agent {
                    name: "Руководитель (AGY)".into(),
                    server: s.id.clone(),
                    backend: "agy".into(),
                    model: "inherit".into(),
                    effort: "default".into(),
                    mode: String::new(),
                    prompt: "Ты руководитель смешанного оркестра. Назначай разработку и ревью профильным исполнителям через relay_cluster.".into(),
                    ..Default::default()
                };
                c.workers = vec![
                    Agent {
                        name: "Кодер (Codex)".into(),
                        server: s.id.clone(),
                        backend: "codex".into(),
                        model: "inherit".into(),
                        effort: "high".into(),
                        mode: "auto".into(),
                        prompt: "Выполни инженерную задачу и прогони релевантные тесты.".into(),
                        ..Default::default()
                    },
                    Agent {
                        name: "Ревьюер (Claude)".into(),
                        server: s.id.clone(),
                        backend: "claude".into(),
                        model: "inherit".into(),
                        effort: "high".into(),
                        mode: "plan".into(),
                        prompt: "Проведи код-ревью, проверь граничные случаи и отчитайся фактами.".into(),
                        ..Default::default()
                    },
                ];
            }
            _ => {
                c.name = "Команда разработки".into();
                c.leader.server = s.id.clone();
                c.leader.backend = String::new();
                c.leader.model = "inherit".into();
                c.leader.effort = "inherit".into();
                c.workers = (0..2)
                    .map(|i| Agent {
                        name: format!("Исполнитель {}", i + 1),
                        server: s.id.clone(),
                        backend: String::new(),
                        model: "inherit".into(),
                        effort: "inherit".into(),
                        ..Default::default()
                    })
                    .collect();
            }
        }
        c
    }
    fn request_catalog_for(&self, server_id: &str, backend: &str) {
        if !self.connected(server_id) || !matches!(backend, "claude" | "codex" | "agy") {
            return;
        }
        self.controller
            .send(server_id, json!({"op": "catalog", "backend": backend}));
    }
    fn request_cluster_catalogs(&self, config: &Config) {
        let mut requested = std::collections::HashSet::new();
        for a in std::iter::once(&config.leader).chain(config.workers.iter()) {
            if a.server.is_empty() || !self.connected(&a.server) {
                continue;
            }
            if let Some(s) = self.settings.servers.iter().find(|s| s.id == a.server) {
                let eff = super::cluster_runtime::agent_backend(a, s);
                if requested.insert((a.server.clone(), eff.to_owned())) {
                    self.request_catalog_for(&a.server, eff);
                }
            }
        }
        if !config.leader.server.is_empty() && self.connected(&config.leader.server) {
            for b in ["agy", "codex", "claude"] {
                if requested.insert((config.leader.server.clone(), b.to_owned())) {
                    self.request_catalog_for(&config.leader.server, b);
                }
            }
        }
    }
    pub(super) fn open_chat_orchestra_dialog(&mut self, server: &Server, k: &str) {
        let fallback_sid = k
            .strip_prefix(&format!("{}/", server.id))
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
        let Some(sid) = self
            .chats
            .get(k)
            .map(|c| c.sid.clone())
            .or(fallback_sid)
        else {
            return;
        };
        if self.connected(&server.id) {
            for b in ["agy", "claude", "codex"] {
                self.request_catalog_for(&server.id, b);
            }
        }
        let existing_id = self.any_cluster_for_leader(&server.id, &sid);
        let existing_run = existing_id
            .as_ref()
            .and_then(|id| self.cluster_store.runs.iter().find(|r| &r.id == id));
        let (palette, max_parallel, auto_results, active_run_id, has_existing_workers) =
            if let Some(r) = existing_run {
                (
                    r.config.model_palette.clone(),
                    r.config.max_parallel.clamp(1, 8),
                    r.config.auto_results,
                    (!r.detached).then(|| r.id.clone()),
                    r.config.workers.iter().any(|w| w.enabled),
                )
            } else {
                (
                    self.settings.chat_orchestra_palette.clone(),
                    self.settings.chat_orchestra_parallel.clamp(1, 8),
                    true,
                    None,
                    false,
                )
            };
        let show_tiers = palette.coder.is_some()
            || palette.reviewer.is_some()
            || palette.researcher.is_some()
            || palette.fast.is_some();
        self.chat_orchestra_dialog = Some(ChatOrchestraDialog {
            server_id: server.id.clone(),
            chat_key: k.to_owned(),
            sid,
            palette,
            max_parallel,
            auto_results,
            active_run_id,
            has_existing_workers,
            update_existing: has_existing_workers,
            show_tiers,
        });
    }
    pub(super) fn chat_orchestra_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.chat_orchestra_dialog.take() else {
            return;
        };
        let Some(server) = self
            .settings
            .servers
            .iter()
            .find(|s| s.id == d.server_id)
            .cloned()
        else {
            return;
        };
        let p = design::animated_palette(ctx, self.settings.light);
        let light = self.settings.light;
        let online = self.connected(&server.id);
        let fallback_run = self
            .any_cluster_for_leader(&server.id, &d.sid)
            .and_then(|id| self.cluster_store.runs.iter().find(|r| r.id == id));
        let chat_backend = if self.chats.contains_key(&d.chat_key) {
            self.backend_for_chat(&server, &d.chat_key)
        } else if let Some(r) = fallback_run {
            super::cluster_runtime::agent_backend(&r.config.leader, &server).to_owned()
        } else {
            server.effective_backend().to_owned()
        };
        let chat_models = self.models_for_backend(&server, &chat_backend);
        let (chat_model, chat_mode) = self
            .chats
            .get(&d.chat_key)
            .map(|c| (c.model.clone(), c.mode.clone()))
            .or_else(|| {
                fallback_run.map(|r| (r.config.leader.model.clone(), r.config.mode.clone()))
            })
            .unwrap_or_else(|| ("default".into(), "auto".into()));
        let mut snap = ServerCatalogSnapshot {
            connected: online,
            claude: self.models_for_backend(&server, "claude"),
            codex: self.models_for_backend(&server, "codex"),
            agy: self.models_for_backend(&server, "agy"),
            api: self.models_for_backend(&server, "api"),
        };

        let mut open = true;
        let mut apply = false;
        let mut apply_and_open_cluster = false;
        let mut disable = false;
        let mut refresh_requests: Vec<String> = Vec::new();

        egui::Window::new("Оркестр в этом чате")
            .id(egui::Id::new("chat-orchestra-dialog"))
            .open(&mut open)
            .default_width(580.)
            .default_pos(egui::pos2(
                ((ctx.screen_rect().width() - 600.) / 2.).max(12.),
                54.,
            ))
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_min_width(ui.available_width().min(540.));
                label(
                    ui,
                    "Оркестратор сам анализирует текущую сессию, создаёт нужных исполнителей и раздаёт им задачи. Задай только модели агентов.",
                    12.,
                    p.muted,
                );
                ui.add_space(10.);

                // Leader summary bar
                let lead_col = design::backend_color(&chat_backend, light);
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, design::mix(p.line, lead_col, 0.35)))
                    .corner_radius(10.)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            label(ui, "Руководитель (этот чат):", 12.5, lead_col);
                            design::mini_pill(
                                ui,
                                Some(design::backend_icon(&chat_backend)),
                                crate::model::backend_short_label(&chat_backend),
                                lead_col,
                                p,
                            );
                            let m_disp = if model::explicit_model(&chat_model) {
                                super::model_label(&chat_models, &chat_model)
                            } else {
                                "выбирается в чате".into()
                            };
                            label(ui, m_disp, 12.5, p.text);
                            design::mini_pill(ui, None, mode_label(&chat_mode), p.muted, p);
                        });
                        label(
                            ui,
                            "Модель руководителя меняется обычным селектором внизу чата.",
                            11.,
                            p.muted,
                        );
                    });

                ui.add_space(10.);
                egui::ScrollArea::vertical()
                    .id_salt("chat-orchestra-scroll")
                    .max_height((ctx.screen_rect().height() - 280.).clamp(200., 520.))
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.line))
                            .corner_radius(10.)
                            .inner_margin(10)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                label(ui, "Основная модель агентов (по умолчанию)", 13.5, p.text);
                                ui.add_space(4.);
                                model_slot_editor(
                                    ui,
                                    "default",
                                    &mut d.palette.default_worker,
                                    &server,
                                    &chat_backend,
                                    &mut snap,
                                    p,
                                    &mut refresh_requests,
                                );
                            });

                        ui.add_space(8.);
                        ui.checkbox(
                            &mut d.show_tiers,
                            "Задать отдельные модели для ролей (Кодер / Ревьюер / Исследователь / Быстрый)",
                        );
                        if d.show_tiers {
                            ui.add_space(6.);
                            for (tier_id, tier_title, tier_hint, slot_opt) in [
                                ("coder", "Кодер", "реализация, правки кода, тесты", &mut d.palette.coder),
                                ("reviewer", "Ревьюер (План)", "аудит изменений и поиск багов", &mut d.palette.reviewer),
                                ("researcher", "Исследователь", "изучение кода, поиск, схема", &mut d.palette.researcher),
                                ("fast", "Быстрый", "мелкие поручения и проверки", &mut d.palette.fast),
                            ] {
                                let mut enabled = slot_opt.is_some();
                                egui::Frame::new()
                                    .fill(design::mix(p.side, p.surface, 0.5))
                                    .stroke(Stroke::new(1.0_f32, p.line))
                                    .corner_radius(9.)
                                    .inner_margin(8)
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal_wrapped(|ui| {
                                            if ui.checkbox(&mut enabled, tier_title).changed() {
                                                if enabled && slot_opt.is_none() {
                                                    *slot_opt = Some(d.palette.default_worker.clone());
                                                } else if !enabled {
                                                    *slot_opt = None;
                                                }
                                            }
                                            label(ui, tier_hint, 11., p.muted);
                                        });
                                        if let Some(slot) = slot_opt.as_mut() {
                                            ui.add_space(4.);
                                            model_slot_editor(
                                                ui,
                                                tier_id,
                                                slot,
                                                &server,
                                                &chat_backend,
                                                &mut snap,
                                                p,
                                                &mut refresh_requests,
                                            );
                                        }
                                    });
                                ui.add_space(5.);
                            }
                        }

                        ui.add_space(6.);
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::Slider::new(&mut d.max_parallel, 1..=8)
                                    .text("Агентов одновременно"),
                            );
                        });
                        ui.checkbox(
                            &mut d.auto_results,
                            "Автоматически возвращать результаты в этот чат, когда агенты закончат",
                        );
                        if d.has_existing_workers {
                            ui.checkbox(
                                &mut d.update_existing,
                                "Применить новые модели к уже созданным агентам этой сессии",
                            );
                        }
                    });

                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    let primary_lbl = if d.active_run_id.is_some() {
                        "Применить модели"
                    } else {
                        "Включить оркестр"
                    };
                    if design::button(ui, primary_lbl, Some(Icon::Check), Kind::Primary, p, Vec2::ZERO)
                        .clicked()
                    {
                        apply = true;
                    }
                    if design::button(
                        ui,
                        "Во вкладку Кластер ↗",
                        Some(Icon::Cluster),
                        Kind::Secondary,
                        p,
                        Vec2::ZERO,
                    )
                    .clicked()
                    {
                        apply = true;
                        apply_and_open_cluster = true;
                    }
                    if d.active_run_id.is_some()
                        && design::button(
                            ui,
                            "Выключить в этом чате",
                            Some(Icon::Close),
                            Kind::Ghost,
                            p,
                            Vec2::ZERO,
                        )
                        .clicked()
                    {
                        disable = true;
                    }
                });
            });

        for b in refresh_requests {
            self.request_catalog_for(&server.id, &b);
        }
        if disable {
            self.disable_chat_orchestra(&d.server_id, &d.sid);
            return;
        }
        if apply {
            let rid = self.enable_chat_orchestra(
                &server,
                &d.chat_key,
                d.palette,
                d.max_parallel,
                d.auto_results,
                d.update_existing,
            );
            if apply_and_open_cluster {
                if let Some(rid) = rid {
                    self.cluster_store.selected = Some(rid);
                }
                self.cluster_open = true;
                self.board_open = false;
                self.settings_open = false;
            }
            return;
        }
        if open {
            self.chat_orchestra_dialog = Some(d);
        }
    }
    fn edit_cluster_config(&mut self, config: Config) {
        let is_new = !self.settings.orchestrators.iter().any(|c| c.id == config.id);
        self.request_cluster_catalogs(&config);
        self.cluster_edit = Some(Editor {
            config,
            is_new,
            tab: 0,
            picked: 0,
            error: String::new(),
            snippets: self.settings.prompt_snippets.clone(),
            search: String::new(),
            preview: false,
        });
    }
    pub(super) fn cluster_panel(&mut self, ctx: &egui::Context) {
        if self.demo && self.cluster_store.runs.is_empty() {
            self.seed_cluster_demo();
        }
        let p = design::animated_palette(ctx, self.settings.light);
        let selected = self.cluster_store.selected.clone();
        let valid = self
            .cluster_snapshot
            .as_ref()
            .is_some_and(|(revision, id, _)| {
                *revision == self.cluster_store.revision && Some(id) == selected.as_ref()
            });
        if !valid {
            self.cluster_snapshot = selected.as_ref().and_then(|id| {
                self.cluster_store
                    .runs
                    .iter()
                    .find(|r| &r.id == id)
                    .map(|r| {
                        (
                            self.cluster_store.revision,
                            id.clone(),
                            std::sync::Arc::new(r.clone()),
                        )
                    })
            });
        }
        let run = self
            .cluster_snapshot
            .as_ref()
            .map(|(_, _, r)| std::sync::Arc::clone(r));
        if self.cluster_target.is_empty() {
            self.cluster_target = self
                .settings
                .orchestrators
                .first()
                .map(|c| c.id.clone())
                .unwrap_or_default();
        }
        let fallback = if run.is_none() {
            self.settings
                .orchestrators
                .iter()
                .find(|c| c.id == self.cluster_target)
                .cloned()
        } else {
            None
        };
        let config = run.as_ref().map(|r| &r.config).or(fallback.as_ref());
        let mut edit = false;
        let mut new_config: Option<&str> = None;
        let mut clone_config = false;
        let mut delete_config: Option<String> = None;
        let mut new_session = false;
        let mut connect = false;
        let mut start = false;
        let mut open = None;
        let mut pause = false;
        let mut stop = false;
        let mut open_chat_models: Option<(String, String)> = None;
        let delete_confirm_id = egui::Id::new("cluster-delete-confirm");
        let mut confirm_delete_for: Option<String> =
            ctx.data(|d| d.get_temp::<Option<String>>(delete_confirm_id)).flatten();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.base).inner_margin(24)).show(ctx,|ui| {
            design::ambient_horizon(ui.painter(), ui.max_rect().expand(24.), accent(), self.settings.light);
            ui.horizontal_wrapped(|ui| {
                let (orb, _) = ui.allocate_exact_size(Vec2::splat(26.), egui::Sense::hover());
                let any_active = run.as_ref().is_some_and(|r| {
                    !r.paused && (r.tasks.iter().any(|t| t.state.active()) || self.chats.get(&key(&r.config.leader.server, &r.leader_sid)).is_some_and(|c| c.busy))
                });
                design::orbital_core(ui, orb, accent(), any_active);
                label(ui,"Команда агентов",23.,p.text);
                if self.demo{design::mini_pill(ui, Some(Icon::Spark), "Предпросмотр", accent(), p);}
                if design::button(ui,"Оркестр AGY",Some(Icon::Orbit),Kind::Primary,p,Vec2::ZERO).clicked(){new_config=Some("agy");}
                if design::button(ui,"Новая команда",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){new_config=Some("default");}
                if config.is_some()&&design::button(ui,"Настроить",Some(Icon::Settings),Kind::Ghost,p,Vec2::ZERO).clicked(){edit=true;}
                if config.is_some()&&design::button(ui,"Дублировать",Some(Icon::Copy),Kind::Ghost,p,Vec2::ZERO).clicked(){clone_config=true;}
                if let Some(c) = config {
                    let saved = self.settings.orchestrators.iter().any(|o| o.id == c.id);
                    if saved {
                        if confirm_delete_for.as_deref() == Some(c.id.as_str()) {
                            if design::button(ui,"Удалить точно",Some(Icon::Trash),Kind::Primary,p,Vec2::ZERO).clicked(){
                                delete_config = Some(c.id.clone());
                                confirm_delete_for = None;
                            }
                            if design::button(ui,"Отмена",None,Kind::Ghost,p,Vec2::ZERO).clicked(){
                                confirm_delete_for = None;
                            }
                        } else if design::button(ui,"Удалить",Some(Icon::Trash),Kind::Ghost,p,Vec2::ZERO).clicked(){
                            confirm_delete_for = Some(c.id.clone());
                        }
                    }
                }
            });
            label(ui,"Живая оркестрация: руководитель распределяет задачи по серверам, движкам и конкретным моделям",12.,p.muted);
            if !self.notice.is_empty(){egui::Frame::new().fill(p.surface).corner_radius(9.).inner_margin(10).show(ui,|ui|{ui.horizontal_wrapped(|ui|{label(ui,&self.notice,12.,p.text);if ui.small_button("Закрыть").clicked(){self.notice.clear();}});});}
            ui.add_space(12.);
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("cluster-config").icon(design::combo_icon)
                    .selected_text(config.as_ref().map(|c|c.name.as_str()).unwrap_or("Выбрать команду"))
                    .show_ui(ui,|ui|{for c in &self.settings.orchestrators {if ui.selectable_value(&mut self.cluster_target,c.id.clone(),&c.name).clicked(){self.cluster_store.selected=None;confirm_delete_for=None;}}});
                egui::ComboBox::from_id_salt("cluster-run").icon(design::combo_icon)
                    .selected_text(run.as_ref().map(|r| {
                        let prefix = if r.config.dynamic_workers { "⚡ " } else { "" };
                        format!("{prefix}{}", model::shorten(&r.title, 36))
                    }).unwrap_or("Новая сессия".into()))
                    .show_ui(ui,|ui| {for r in self.cluster_store.runs.iter().rev(){
                        let prefix = if r.config.dynamic_workers { "⚡ " } else { "" };
                        ui.selectable_value(&mut self.cluster_store.selected,Some(r.id.clone()),format!("{prefix}{} · {}",r.config.name,model::shorten(&r.title,52)));
                    }});
                if design::button(ui,"Новая сессия",Some(Icon::Plus),Kind::Ghost,p,Vec2::ZERO).clicked(){new_session=true;}
            });
            ui.add_space(12.);
            egui::ScrollArea::vertical().id_salt("orchestrator-dashboard").show(ui,|ui| {
                if let Some(c)=config {
                    let light = self.settings.light;
                    let enabled_workers = c.workers.iter().filter(|w| w.enabled).count();
                    let active_tasks = run.as_ref().map(|r| r.tasks.iter().filter(|t| t.state.active()).count()).unwrap_or(0);
                    let done_tasks = run.as_ref().map(|r| r.tasks.iter().filter(|t| t.state == TaskState::Done).count()).unwrap_or(0);
                    let total_tasks = run.as_ref().map(|r| r.tasks.len()).unwrap_or(0);

                    // Mission Telemetry Ribbon
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8., 6.);
                        if c.dynamic_workers {
                            design::mini_pill(
                                ui,
                                Some(Icon::Spark),
                                "Динамический оркестр из чата",
                                accent(),
                                p,
                            );
                        }
                        design::mini_pill(
                            ui,
                            Some(Icon::Cluster),
                            &format!("Исполнителей: {enabled_workers}"),
                            accent(),
                            p,
                        );
                        design::mini_pill(
                            ui,
                            Some(Icon::Pulse),
                            &format!("Активно: {active_tasks} / {}", c.max_parallel),
                            if active_tasks > 0 { accent() } else { p.muted },
                            p,
                        );
                        design::mini_pill(
                            ui,
                            Some(Icon::Check),
                            &format!("Завершено: {done_tasks} из {total_tasks}"),
                            if done_tasks > 0 { GREEN } else { p.muted },
                            p,
                        );
                        design::mini_pill(
                            ui,
                            Some(Icon::Layers),
                            &format!("Режим: {}", mode_label(&c.mode)),
                            p.muted,
                            p,
                        );
                    });
                    ui.add_space(10.);

                    let worker_nodes: Vec<_> = c
                        .workers
                        .iter()
                        .filter(|w| w.enabled)
                        .map(|a| build_node_telemetry(self, run.as_deref(), a, &c.mode, false, light))
                        .collect();
                    let leader_node = build_node_telemetry(self, run.as_deref(), &c.leader, &c.mode, true, light);
                    if let Some(clicked) = draw_graph(ui, &leader_node, &worker_nodes, light, p) {
                        if let Some(r)=&run {
                            if clicked=="leader" {open=Some((r.config.leader.server.clone(),r.leader_sid.clone()));}
                            else if let Some(t)=r.tasks.iter().rev().find(|t|t.agent.id==clicked){open=Some((t.agent.server.clone(),t.sid.clone()));}
                            else { edit = true; }
                        } else {
                            edit = true;
                        }
                    }
                    if c.dynamic_workers && worker_nodes.is_empty() {
                        ui.add_space(6.);
                        label(
                            ui,
                            "Агенты ещё не созданы — как только ты дашь задачу в чате, оркестратор сам создаст нужных исполнителей по выбранным моделям, и они появятся на схеме.",
                            12.,
                            p.muted,
                        );
                    }
                    ui.add_space(10.);
                    ui.horizontal_wrapped(|ui| {
                        if design::button(ui,"Подключить хосты",Some(Icon::Server),Kind::Secondary,p,Vec2::ZERO).clicked(){connect=true;}
                        if let Some(r)=&run {
                            if design::button(ui,"К руководителю",Some(Icon::Cluster),Kind::Primary,p,Vec2::ZERO).clicked(){open=Some((r.config.leader.server.clone(),r.leader_sid.clone()));}
                            if r.config.dynamic_workers
                                && design::button(ui,"Модели агентов",Some(Icon::Orbit),Kind::Secondary,p,Vec2::ZERO).clicked()
                            {
                                open_chat_models = Some((r.config.leader.server.clone(), key(&r.config.leader.server, &r.leader_sid)));
                            }
                            if design::button(ui,if r.paused{"Продолжить очередь"}else{"Пауза очереди"},None,Kind::Secondary,p,Vec2::ZERO).clicked(){pause=true;}
                            if design::button(ui,"Остановить команду",Some(Icon::Stop),Kind::Ghost,p,Vec2::ZERO).clicked(){stop=true;}
                        }
                    });
                    if let Some(r)=&run{if r.paused{label(ui,"Очередь на паузе. Сохранённые результаты доступны руководителю.",12.,p.muted);}}
                    ui.add_space(16.);
                    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32,p.line)).corner_radius(14.).inner_margin(14).show(ui,|ui| {
                        ui.set_width(ui.available_width());
                        label(ui,if run.is_some(){"Сообщение руководителю"}else{"Цель для всей команды"},14.,p.text);
                        ui.add(egui::TextEdit::multiline(&mut self.cluster_prompt).id(egui::Id::new("cluster-goal-input")).desired_width(f32::INFINITY).desired_rows(3).frame(false)
                            .hint_text("Опиши результат, ограничения и условия готовности…"));
                        let vars=templates::variables(&self.settings.servers,&self.settings.prompt_snippets,&Context::default()).into_iter().filter(|v|v.token!="[задача]").collect::<Vec<_>>();
                        variable_picker(ui,&mut self.cluster_prompt,egui::Id::new("cluster-goal-input"),&vars,p);
                        ui.add_enabled_ui(!self.cluster_prompt.trim().is_empty(),|ui| {
                            if design::button(ui,if run.is_some(){"Отправить руководителю"}else{"Запустить сессию кластера"},Some(Icon::Send),Kind::Primary,p,Vec2::ZERO).clicked(){start=true;}
                        });
                        label(ui,format!("До {} задач одновременно · до {} назначений · {} исполнителей",c.max_parallel,c.max_tasks,enabled_workers),11.,p.muted);
                    });
                    if let Some(r)=&run {
                        ui.add_space(20.);label(ui,"Задачи и результаты",16.,p.text);
                        if r.tasks.is_empty(){label(ui,"Задачи появятся, когда руководитель распределит работу.",12.,p.muted);}
                        for t in r.tasks.iter().rev() {
                            let srv = self.settings.servers.iter().find(|s| s.id == t.agent.server);
                            let eff_b = srv
                                .map(|s| super::cluster_runtime::agent_backend(&t.agent, s))
                                .unwrap_or_else(|| if crate::model::valid_backend(&t.agent.backend) { t.agent.backend.as_str() } else { "claude" });
                            let eff_model_label = agent_resolved_model_display(self, &t.agent, srv, eff_b);
                            let b_col = design::backend_color(eff_b, light);
                            let st_col = state_color(t.state, p);
                            egui::Frame::new()
                                .fill(design::mix(p.surface, st_col, if t.state.active() { 0.06 } else { 0.02 }))
                                .stroke(Stroke::new(1.0_f32, design::mix(p.line, st_col, if t.state.active() { 0.45 } else { 0.15 })))
                                .corner_radius(12.)
                                .inner_margin(13)
                                .show(ui,|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        design::mini_pill(ui, Some(if t.state == TaskState::Done { Icon::Check } else { Icon::Pulse }), t.state.label(), st_col, p);
                                        design::mini_pill(ui, Some(design::backend_icon(eff_b)), crate::model::backend_short_label(eff_b), b_col, p);
                                        design::mini_pill(ui, None, &eff_model_label, p.muted, p);
                                        label(ui,&t.agent.name,12.5,p.text);
                                        if t.delivered {
                                            design::mini_pill(ui, Some(Icon::Check), "Передано руководителю", GREEN, p);
                                        }
                                        if design::button(ui,"Открыть сессию",Some(Icon::Arrow),Kind::Ghost,p,Vec2::ZERO).clicked(){open=Some((t.agent.server.clone(),t.sid.clone()));}
                                    });
                                    ui.add_space(4.);
                                    ui.add(egui::Label::new(RichText::new(&t.title).size(14.).color(p.text)).wrap());
                                    if !t.error.is_empty(){label(ui,model::shorten(&t.error,240),12.,Color32::from_rgb(228,126,136));}
                                    if !t.result.is_empty(){label(ui,model::shorten(&t.result,350),12.,p.muted);}
                                });
                            ui.add_space(7.);
                        }
                        ui.add_space(18.);label(ui,"Журнал оркестрации",15.,p.text);
                        for entry in r.journal.iter().rev().take(12) {
                            ui.horizontal(|ui| {
                                let (dr, _) = ui.allocate_exact_size(Vec2::splat(10.), egui::Sense::hover());
                                ui.painter().circle_filled(dr.center(), 2.5, accent().gamma_multiply(0.7));
                                label(ui,&entry.message,12.,p.muted);
                            });
                        }
                    }
                }else{
                    ui.add_space(28.);
                    label(ui,"Собери свой оркестр агентов",21.,p.text);
                    label(ui,"На каждом агенте (руководителе и исполнителях) можно задать хост, движок (AGY, Claude, Codex, API), конкретную модель или авто-выбор движка, глубину и права.",13.,p.muted);
                    ui.add_space(16.);
                    ui.horizontal_wrapped(|ui| {
                        if design::button(ui,"Создать оркестр AGY",Some(Icon::Orbit),Kind::Primary,p,Vec2::ZERO).clicked(){new_config=Some("agy");}
                        if design::button(ui,"Смешанный (AGY + Codex + Claude)",Some(Icon::Cluster),Kind::Secondary,p,Vec2::ZERO).clicked(){new_config=Some("mixed");}
                        if design::button(ui,"По профилю хоста",Some(Icon::Plus),Kind::Ghost,p,Vec2::ZERO).clicked(){new_config=Some("classic");}
                    });
                }
            });
        });
        if let Some((srv_id, chat_k)) = open_chat_models {
            if let Some(s) = self.settings.servers.iter().find(|s| s.id == srv_id).cloned() {
                self.open_chat_orchestra_dialog(&s, &chat_k);
            }
        }
        ctx.data_mut(|d| d.insert_temp(delete_confirm_id, confirm_delete_for));
        if let Some(kind) = new_config {
            let cfg = if kind == "default" {
                self.default_cluster_config()
            } else {
                self.preset_cluster_config(kind)
            };
            self.edit_cluster_config(cfg);
        }
        if clone_config {
            if let Some(c) = config {
                self.edit_cluster_config(c.clone_as_new());
            }
        }
        if let Some(del_id) = delete_config {
            self.settings.orchestrators.retain(|c| c.id != del_id);
            if self.cluster_target == del_id {
                self.cluster_target = self
                    .settings
                    .orchestrators
                    .first()
                    .map(|c| c.id.clone())
                    .unwrap_or_default();
            }
            if self
                .cluster_store
                .selected
                .as_ref()
                .and_then(|id| self.cluster_store.runs.iter().find(|r| &r.id == id))
                .is_some_and(|r| r.config.id == del_id)
            {
                self.cluster_store.selected = None;
            }
            self.save();
            self.notice = "Команда удалена.".into();
        }
        if edit {
            if let Some(c) = config {
                self.edit_cluster_config(Config::clone(c));
            }
        }
        if new_session {
            if let Some(c) = config {
                self.cluster_target = c.id.clone();
            }
            self.cluster_store.selected = None;
            self.cluster_store.dirty = true;
        }
        if connect {
            if let Some(c) = config {
                let ids: std::collections::HashSet<_> = std::iter::once(&c.leader)
                    .chain(c.workers.iter().filter(|a| a.enabled))
                    .map(|a| a.server.clone())
                    .collect();
                for id in ids {
                    if !self.connected(&id) && !self.remote.get(&id).is_some_and(|r| r.connecting) {
                        if let Some(s) = self.settings.servers.iter().find(|s| s.id == id).cloned()
                        {
                            self.connect(s);
                        }
                    }
                }
            }
        }
        if let Some((server, sid)) = open {
            self.open_cluster_chat(&server, &sid);
        }
        if pause {
            if let Some(id) = &selected {
                if let Some(r) = self.cluster_store.runs.iter_mut().find(|r| &r.id == id) {
                    r.paused = !r.paused;
                    if !r.paused {
                        if let Some(c) = self
                            .chats
                            .get_mut(&key(&r.config.leader.server, &r.leader_sid))
                        {
                            c.queue_paused = false;
                        }
                    }
                    r.log(if r.paused {
                        "Очередь приостановлена"
                    } else {
                        "Очередь продолжена"
                    });
                    self.cluster_store.dirty = true;
                }
            }
        }
        if stop {
            if let Some(id) = &selected {
                self.stop_cluster(id);
            }
        }
        if start {
            if let Some(r) = run.as_ref() {
                let server_id = r.config.leader.server.clone();
                let sid = r.leader_sid.clone();
                if let Some(server) = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server_id)
                    .cloned()
                {
                    let k = key(&server_id, &sid);
                    if let Some(c) = self.chats.get_mut(&k) {
                        if c.draft.is_empty() {
                            c.draft = std::mem::take(&mut self.cluster_prompt);
                            if c.busy {
                                c.enqueue_draft();
                            } else {
                                self.send_to_chat(server, k);
                            }
                        } else {
                            self.notice =
                                "У руководителя есть черновик. Открой сессию, чтобы продолжить."
                                    .into();
                        }
                    }
                }
            } else if let Some(c) = config {
                if let Err(e) = self.start_cluster(Config::clone(c), self.cluster_prompt.clone()) {
                    self.notice = e;
                }
            }
        }
    }
    pub(super) fn cluster_settings(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.cluster_edit.take() else {
            return;
        };
        let p = design::animated_palette(ctx, self.settings.light);
        let light = self.settings.light;
        let catalogs = build_editor_catalogs(self);
        let mut open = true;
        let mut save = false;
        let mut refresh_requests: Vec<(String, String)> = Vec::new();
        let mut apply_preset: Option<&str> = None;
        egui::Window::new("Настройка агентского кластера").id(egui::Id::new("cluster-editor")).open(&mut open)
            .default_width(820.).default_height(700.).default_pos(egui::pos2(((ctx.screen_rect().width()-840.)/2.).max(8.),30.))
            .resizable(true).collapsible(false).show(ctx,|ui| {
                ui.horizontal_wrapped(|ui|{
                    for (i,name) in ["Состав и модели","Промпты","Лимиты","Вставки"].iter().enumerate(){
                        if ui.selectable_label(e.tab==i,*name).clicked(){e.tab=i;}
                    }
                    if e.is_new && e.tab == 0 {
                        ui.separator();
                        label(ui, "Шаблон:", 11.5, p.muted);
                        if ui.small_button("Оркестр AGY").clicked() { apply_preset = Some("agy"); }
                        if ui.small_button("Смешанный").clicked() { apply_preset = Some("mixed"); }
                        if ui.small_button("По хосту").clicked() { apply_preset = Some("classic"); }
                    }
                });
                ui.add_space(12.);
                egui::ScrollArea::vertical().id_salt("cluster-editor-scroll").max_height((ctx.screen_rect().height()-230.).clamp(220.,660.)).show(ui,|ui| {
                    ui.set_min_width(ui.available_width().min(740.));
                    match e.tab {
                        0=>{
                            label(ui,"Название команды",12.,p.muted);
                            ui.add(egui::TextEdit::singleline(&mut e.config.name).desired_width(f32::INFINITY));
                            ui.add_space(10.);
                            let _ = agent_editor(
                                ui,
                                &mut e.config.leader,
                                &self.settings.servers,
                                &catalogs,
                                &e.config.mode,
                                None,
                                0,
                                true,
                                light,
                                p,
                                &mut refresh_requests,
                            );
                            let mut action: Option<WorkerCardAction> = None;
                            let worker_count = e.config.workers.len();
                            for (i,a) in e.config.workers.iter_mut().enumerate(){
                                ui.add_space(10.);
                                ui.push_id(i,|ui|{
                                    if let Some(act) = agent_editor(
                                        ui,
                                        a,
                                        &self.settings.servers,
                                        &catalogs,
                                        &e.config.mode,
                                        Some(i),
                                        worker_count,
                                        false,
                                        light,
                                        p,
                                        &mut refresh_requests,
                                    ) {
                                        action = Some(act);
                                    }
                                });
                            }
                            if let Some(act) = action {
                                match act {
                                    WorkerCardAction::Remove(i) => { e.config.workers.remove(i); }
                                    WorkerCardAction::MoveUp(i) if i > 0 => { e.config.workers.swap(i - 1, i); }
                                    WorkerCardAction::MoveDown(i) if i + 1 < e.config.workers.len() => { e.config.workers.swap(i, i + 1); }
                                    WorkerCardAction::Duplicate(i) if e.config.workers.len() < 12 => {
                                        let mut copy = e.config.workers[i].clone();
                                        copy.id = crate::cluster::id();
                                        copy.name = format!("{} (2)", copy.name.trim());
                                        e.config.workers.insert(i + 1, copy);
                                    }
                                    _ => {}
                                }
                            }
                            ui.add_space(10.);
                            if e.config.workers.len()<12&&design::button(ui,"Добавить исполнителя",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){
                                e.config.workers.push(Agent{
                                    name:format!("Исполнитель {}",e.config.workers.len()+1),
                                    server:e.config.leader.server.clone(),
                                    backend:e.config.leader.backend.clone(),
                                    model:"inherit".into(),
                                    effort:if e.config.leader.backend == "agy" { "default".into() } else { "inherit".into() },
                                    mode:String::new(),
                                    ..Default::default()
                                });
                            }
                        }
                        1=>{
                            let names:Vec<_>=std::iter::once("Общие инструкции команды".into()).chain(std::iter::once(format!("Руководитель · {}",e.config.leader.name))).chain(e.config.workers.iter().map(|a|format!("Исполнитель · {}",a.name))).collect();
                            e.picked=e.picked.min(names.len()-1);
                            egui::ComboBox::from_id_salt("prompt-agent").icon(design::combo_icon).selected_text(&names[e.picked]).show_ui(ui,|ui|{for (i,name) in names.iter().enumerate(){ui.selectable_value(&mut e.picked,i,name);}});
                            let agent=if e.picked<=1{e.config.leader.clone()}else{e.config.workers[e.picked-2].clone()};
                            let server=self.settings.servers.iter().find(|s|s.id==agent.server);
                            let cwd=server.map(|s|super::cluster_runtime::agent_cwd(&agent,s)).unwrap_or_default();
                            let context=Context{cluster:&e.config.name,agent:&agent.name,task:"Пример задачи для предпросмотра",host:server.map(|s|s.host.as_str()).unwrap_or(""),cwd:&cwd};
                            let vars=templates::variables(&self.settings.servers,&e.snippets,&context);
                            let field=egui::Id::new(("custom-cluster-prompt",&e.config.id,e.picked));
                            let text=if e.picked==0{&mut e.config.shared_prompt}else if e.picked==1{&mut e.config.leader.prompt}else{&mut e.config.workers[e.picked-2].prompt};
                            label(ui,"Эти инструкции добавляются к системному промпту выбранного агента.",12.,p.muted);
                            ui.add(egui::TextEdit::multiline(text).id(field).desired_rows(12).desired_width(f32::INFINITY).hint_text("Обязанности, ограничения, формат результата и правила проверки…"));
                            variable_picker(ui,text,field,&vars,p);
                            ui.checkbox(&mut e.preview,"Предпросмотр подстановок");
                            if e.preview{match templates::expand(text,&vars){Ok(value)=>{egui::Frame::new().fill(p.base).corner_radius(9.).inner_margin(12).show(ui,|ui|{ui.add(egui::Label::new(RichText::new(value).monospace().size(12.)).wrap().selectable(true));});},Err(error)=>{label(ui,error,12.,Color32::from_rgb(222,143,151));}}}
                        }
                        2=>{
                            label(ui,"Как работает команда",18.,p.text);
                            ui.add(egui::Slider::new(&mut e.config.max_parallel,1..=8).text("Задач одновременно"));
                            ui.add(egui::Slider::new(&mut e.config.max_tasks,1..=128).text("Назначений за сессию"));
                            egui::ComboBox::from_id_salt("cluster-mode").icon(design::combo_icon).selected_text(format!("Базовый режим: {}", mode_label(&e.config.mode))).show_ui(ui,|ui|{for id in ["auto","manual","acceptEdits","plan"]{ui.selectable_value(&mut e.config.mode,id.into(),mode_label(id));}});
                            ui.checkbox(&mut e.config.auto_results,"Возвращать результаты руководителю автоматически, если он завершил ответ раньше исполнителей");
                            label(ui,"Один исполнитель получает по одной задаче. Зависимости и общий предел параллелизма соблюдаются Relay. У любого агента можно переопределить режим прав в карточке состава.",12.,p.muted);
                        }
                        _=>{
                            label(ui,"Данные серверов берутся из сохранённых подключений Relay слева.",12.,p.muted);
                            ui.add(egui::TextEdit::singleline(&mut e.search).hint_text("Найти сервер или ключ").desired_width(f32::INFINITY));
                            let vars=templates::variables(&self.settings.servers,&e.snippets,&Context::default());
                            for v in vars.iter().filter(|v|!v.label.ends_with(" · ID")&&(e.search.is_empty()||format!("{} {}",v.token,v.label).to_lowercase().contains(&e.search.to_lowercase()))) {
                                ui.horizontal_wrapped(|ui|{if ui.small_button("Копировать").clicked(){ctx.copy_text(v.token.clone());}ui.label(RichText::new(&v.token).monospace().size(12.));label(ui,&v.label,11.,p.muted);});
                            }
                            ui.add_space(16.);label(ui,"Мои фрагменты промпта",16.,p.text);
                            let mut remove=None;
                            for (i,s) in e.snippets.iter_mut().enumerate(){ui.push_id(i,|ui|{
                                ui.horizontal(|ui|{label(ui,"Ключ",12.,p.muted);ui.add(egui::TextEdit::singleline(&mut s.key).desired_width(200.));if ui.small_button("Удалить").clicked(){remove=Some(i);}});
                                ui.add(egui::TextEdit::multiline(&mut s.value).desired_rows(3).desired_width(f32::INFINITY).hint_text("Текст, который будет вставлен вместо ключа. Можно использовать данные серверов."));ui.add_space(10.);
                            });}
                            if let Some(i)=remove{e.snippets.remove(i);}
                            if design::button(ui,"Добавить фрагмент",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){e.snippets.push(Snippet{key:format!("мой_контекст_{}",e.snippets.len()+1),value:String::new()});}
                        }
                    }
                });
                ui.add_space(12.);
                if !e.error.is_empty(){label(ui,&e.error,12.,Color32::from_rgb(222,143,151));}
                ui.horizontal(|ui|{
                    if design::button(ui,"Сохранить настройки",Some(Icon::Check),Kind::Primary,p,Vec2::ZERO).clicked(){save=true;}
                    label(ui,"Модели, движки и промпты сохраняются вместе с командой",11.,p.muted);
                });
            });
        if let Some(preset) = apply_preset {
            let keep_id = e.config.id.clone();
            e.config = self.preset_cluster_config(preset);
            e.config.id = keep_id;
            self.request_cluster_catalogs(&e.config);
        }
        for (srv_id, backend) in refresh_requests {
            self.request_catalog_for(&srv_id, &backend);
        }
        if save {
            let validation = e.config.validate().and_then(|_| {
                let mut keys = std::collections::HashSet::new();
                for s in &e.snippets {
                    if s.key.is_empty()
                        || s.key.len() > 128
                        || !s
                            .key
                            .chars()
                            .all(|c| c.is_alphanumeric() || "_.-".contains(c))
                        || !keys.insert(s.key.clone())
                    {
                        return Err("У вставок должны быть разные имена из букв, цифр и _.−".into());
                    }
                }
                let vars =
                    templates::variables(&self.settings.servers, &e.snippets, &Context::default());
                for s in &e.snippets {
                    templates::expand(&format!("[{}]", s.key), &vars)?;
                }
                Ok(())
            });
            match validation {
                Ok(()) => {
                    if let Some(c) = self
                        .settings
                        .orchestrators
                        .iter_mut()
                        .find(|c| c.id == e.config.id)
                    {
                        *c = e.config.clone();
                    } else {
                        self.settings.orchestrators.push(e.config.clone());
                    }
                    self.settings.prompt_snippets = e.snippets.clone();
                    self.cluster_target = e.config.id.clone();
                    if let Some(id) = self.cluster_store.selected.clone() {
                        if let Some(r) = self
                            .cluster_store
                            .runs
                            .iter_mut()
                            .find(|r| r.id == id && r.config.id == e.config.id)
                        {
                            let busy = self
                                .chats
                                .get(&key(&r.config.leader.server, &r.leader_sid))
                                .is_some_and(|c| c.busy)
                                || r.tasks.iter().any(|t| t.state.active());
                            if r.config.leader.server == e.config.leader.server
                                && r.config.leader.cwd == e.config.leader.cwd
                            {
                                r.pending_config = Some(e.config.clone());
                                r.log(if busy {
                                    "Новые промпты применятся после текущей работы"
                                } else {
                                    "Промпты обновлены для следующего сообщения"
                                });
                                self.cluster_store.dirty = true;
                            } else {
                                self.notice="Настройки сохранены для новой сессии команды. Текущая работа продолжается со своим составом.".into();
                            }
                        }
                    }
                    if self
                        .cluster_store
                        .selected
                        .as_ref()
                        .and_then(|id| self.cluster_store.runs.iter().find(|r| &r.id == id))
                        .is_some_and(|r| r.config.id != e.config.id)
                    {
                        self.cluster_store.selected = None;
                    }
                    self.save();
                    open = false;
                }
                Err(error) => e.error = error,
            }
        }
        if open {
            self.cluster_edit = Some(e);
        }
    }
    fn seed_cluster_demo(&mut self) {
        let mut c = self.default_cluster_config();
        c.name = "Команда Relay".into();
        c.leader.name = "Архитектор".into();
        c.leader.backend = "claude".into();
        c.workers = vec![
            Agent {
                id: "ui-worker".into(),
                name: "Интерфейс ПК".into(),
                server: "demo".into(),
                backend: "codex".into(),
                model: "gpt-5.4".into(),
                ..Default::default()
            },
            Agent {
                id: "review-worker".into(),
                name: "Проверка ядра".into(),
                server: "stage".into(),
                backend: "claude".into(),
                model: "sonnet".into(),
                ..Default::default()
            },
            Agent {
                id: "mobile-worker".into(),
                name: "Android и TUI".into(),
                server: "demo".into(),
                backend: "agy".into(),
                model: "gemini-3.1-pro-preview".into(),
                ..Default::default()
            },
        ];
        let mut r = Run::new(
            c.clone(),
            "Собрать визуализацию конвейера агентов и проверить совместимость".into(),
        );
        r.dispatch(
            "one",
            &json!({"worker":"ui-worker","title":"Топология и конвейер шагов","task":"Отрисовать узлы агентов и потоки"}),
        )
        .unwrap();
        r.dispatch(
            "two",
            &json!({"worker":"review-worker","title":"Проверить совместимость","task":"Проверить старые настройки и тесты"}),
        )
        .unwrap();
        r.dispatch(
            "three",
            &json!({"worker":"mobile-worker","title":"Радар агентов на Android","task":"Обновить экраны и шторку радара"}),
        )
        .unwrap();
        r.tasks[0].state = TaskState::Running;
        r.tasks[1].state = TaskState::Done;
        r.tasks[1].result = "Старые профили загружаются. 142 проверки пройдены.".into();
        r.tasks[1].delivered = true;
        r.tasks[2].state = TaskState::Running;

        // Populate live tool telemetry on demo workers so topology cards show real actions
        let mut w1_chat = Chat::new(r.tasks[0].sid.clone(), "/opt/relay".into(), "Интерфейс ПК".into());
        w1_chat.busy = true;
        w1_chat.loaded = true;
        w1_chat.messages = vec![
            Message {
                role: "tool".into(),
                tool: "Edit".into(),
                tool_id: "w1-t1".into(),
                input: json!({"file_path": "/opt/relay/src/timeline.rs", "description": "Конвейер шагов"}),
                ..Default::default()
            },
            Message {
                role: "tool_result".into(),
                tool_id: "w1-t1".into(),
                text: "ok".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Bash".into(),
                tool_id: "w1-t2".into(),
                input: json!({"command": "cargo test --locked", "description": "Проверка Rust-тестов"}),
                ..Default::default()
            },
        ];
        self.chats.insert(key("demo", &r.tasks[0].sid), w1_chat);

        let mut w3_chat = Chat::new(r.tasks[2].sid.clone(), "/opt/relay/android".into(), "Android и TUI".into());
        w3_chat.busy = true;
        w3_chat.loaded = true;
        w3_chat.messages = vec![Message {
            role: "tool".into(),
            tool: "Edit".into(),
            tool_id: "w3-t1".into(),
            input: json!({"file_path": "/opt/relay/android/app/src/main/java/dev/relay/mobile/Ui.kt", "description": "Шторка радара агентов"}),
            ..Default::default()
        }];
        self.chats.insert(key("demo", &r.tasks[2].sid), w3_chat);

        self.settings.orchestrators = vec![c];
        self.cluster_store.selected = Some(r.id.clone());
        self.cluster_store.runs.push(r);
    }
}
fn mode_label(mode: &str) -> &str {
    match mode {
        "auto" => "Авто",
        "acceptEdits" => "Правки файлов",
        "plan" => "Планирование",
        _ => "С подтверждением",
    }
}
enum WorkerCardAction {
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
    Duplicate(usize),
}

struct ServerCatalogSnapshot {
    connected: bool,
    claude: Vec<crate::model::ModelInfo>,
    codex: Vec<crate::model::ModelInfo>,
    agy: Vec<crate::model::ModelInfo>,
    api: Vec<crate::model::ModelInfo>,
}

impl ServerCatalogSnapshot {
    fn for_backend(&self, backend: &str) -> &[crate::model::ModelInfo] {
        match backend {
            "codex" => &self.codex,
            "agy" => &self.agy,
            "api" => &self.api,
            _ => &self.claude,
        }
    }
}

fn build_editor_catalogs(app: &RelayApp) -> HashMap<String, ServerCatalogSnapshot> {
    app.settings
        .servers
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                ServerCatalogSnapshot {
                    connected: app.connected(&s.id),
                    claude: app.models_for_backend(s, "claude"),
                    codex: app.models_for_backend(s, "codex"),
                    agy: app.models_for_backend(s, "agy"),
                    api: app.models_for_backend(s, "api"),
                },
            )
        })
        .collect()
}

fn effort_choices(backend: &str, model: &str, inherited: &str, models: &[crate::model::ModelInfo]) -> impl Iterator<Item=&'static str> {
    let selected = if model == "inherit" || model.is_empty() { inherited } else { model };
    let ultra = backend == "codex" && models.iter().any(|m| m.value == selected && m.ultra);
    crate::model::backend_effort_options(backend).iter().copied().chain(ultra.then_some("ultra"))
}

fn format_model_choice(models: &[crate::model::ModelInfo], value: &str, host_model: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        "Авто движка (не задана)".into()
    } else if trimmed.eq_ignore_ascii_case("inherit") {
        let host_label = if host_model.trim().is_empty() || host_model.eq_ignore_ascii_case("default") {
            "авто".into()
        } else {
            super::model_label(models, host_model)
        };
        format!("По профилю ({host_label})")
    } else {
        super::model_label(models, trimmed)
    }
}

fn agent_resolved_model_display(
    app: &RelayApp,
    agent: &Agent,
    srv: Option<&Server>,
    eff_backend: &str,
) -> String {
    let models = srv
        .map(|s| app.models_for_backend(s, eff_backend))
        .unwrap_or_else(|| crate::model::default_backend_models(eff_backend));
    let resolved = srv
        .map(|s| super::cluster_runtime::agent_model(agent, s))
        .unwrap_or_else(|| agent.model.clone());
    if resolved.trim().is_empty()
        || resolved.eq_ignore_ascii_case("default")
        || resolved.eq_ignore_ascii_case("inherit")
    {
        "авто движка".into()
    } else {
        super::model_label(&models, &resolved)
    }
}

fn effort_display(value: &str, host_effort: &str) -> String {
    match value.trim() {
        "" | "default" => "по умолчанию".into(),
        "inherit" => {
            let he = if host_effort.trim().is_empty() || host_effort == "default" {
                "по умолчанию"
            } else {
                host_effort
            };
            format!("по профилю ({he})")
        }
        other => other.into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn model_slot_editor(
    ui: &mut egui::Ui,
    salt: &str,
    slot: &mut crate::cluster::ModelSlot,
    server: &Server,
    leader_backend: &str,
    snap: &mut ServerCatalogSnapshot,
    p: Palette,
    refresh_requests: &mut Vec<String>,
) {
    let prev_backend = slot.backend.clone();
    let leader_eff = if crate::model::valid_backend(leader_backend) {
        leader_backend
    } else {
        server.effective_backend()
    };
    ui.horizontal_wrapped(|ui| {
        let b_btn = match slot.backend.as_str() {
            "claude" => "Движок: Claude Code".into(),
            "codex" => "Движок: Codex".into(),
            "agy" => "Движок: AGY".into(),
            "api" => "Движок: API".into(),
            _ => format!("Движок: как у чата ({})", crate::model::backend_short_label(leader_eff)),
        };
        egui::ComboBox::from_id_salt(("slot-backend", salt))
            .icon(design::combo_icon)
            .selected_text(b_btn)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut slot.backend,
                    String::new(),
                    format!("Как у чата ({})", crate::model::backend_short_label(leader_eff)),
                );
                for (id, name) in [
                    ("agy", "Antigravity (AGY)"),
                    ("claude", "Claude Code"),
                    ("codex", "Codex"),
                    ("api", "Пользовательский API"),
                ] {
                    ui.selectable_value(&mut slot.backend, id.into(), name);
                }
            });
        let eff_backend = if crate::model::valid_backend(&slot.backend) {
            slot.backend.clone()
        } else {
            leader_eff.to_owned()
        };
        if slot.backend != prev_backend {
            slot.model = "inherit".into();
            slot.effort = if eff_backend == "agy" {
                "default".into()
            } else {
                "inherit".into()
            };
            if snap.connected && matches!(eff_backend.as_str(), "claude" | "codex" | "agy") {
                refresh_requests.push(eff_backend.clone());
            }
        }

        let models = snap.for_backend(&eff_backend);
        let host_model = server.model_for(&eff_backend);
        let model_btn = format!("Модель: {}", format_model_choice(models, &slot.model, host_model));
        egui::ComboBox::from_id_salt(("slot-model", salt))
            .icon(design::combo_icon)
            .selected_text(model_btn)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut slot.model,
                    "inherit".into(),
                    format_model_choice(models, "inherit", host_model),
                );
                ui.selectable_value(
                    &mut slot.model,
                    "default".into(),
                    "Не задавать (авто движка)",
                );
                ui.separator();
                if !models.is_empty() {
                    egui::ScrollArea::vertical()
                        .max_height(220.)
                        .show(ui, |ui| {
                            for m in models.iter().filter(|m| crate::model::explicit_model(&m.value)) {
                                let disp = super::model_label(models, &m.value);
                                ui.selectable_value(&mut slot.model, m.value.clone(), disp)
                                    .on_hover_text(&m.value);
                            }
                        });
                } else if eff_backend == "claude" {
                    for (id, name) in [("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")] {
                        ui.selectable_value(&mut slot.model, id.into(), name);
                    }
                } else {
                    for m in crate::model::default_backend_models(&eff_backend) {
                        ui.selectable_value(&mut slot.model, m.value, m.display_name);
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    label(ui, "Своя:", 11.5, p.muted);
                    ui.add(
                        egui::TextEdit::singleline(&mut slot.model)
                            .desired_width(175.)
                            .hint_text("ID модели"),
                    );
                });
            });

        if snap.connected && matches!(eff_backend.as_str(), "claude" | "codex" | "agy") {
            if ui
                .small_button("↻")
                .on_hover_text("Обновить список моделей с сервера")
                .clicked()
            {
                refresh_requests.push(eff_backend.clone());
            }
        }

        if crate::model::backend_supports_effort(&eff_backend) {
            let host_effort = server.effort.as_str();
            egui::ComboBox::from_id_salt(("slot-effort", salt))
                .icon(design::combo_icon)
                .selected_text(format!("Глубина: {}", effort_display(&slot.effort, host_effort)))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut slot.effort,
                        "inherit".into(),
                        effort_display("inherit", host_effort),
                    );
                    ui.selectable_value(&mut slot.effort, "default".into(), "По умолчанию движка");
                    ui.separator();
                    for id in effort_choices(&eff_backend, &slot.model, host_model, models) {
                        ui.selectable_value(&mut slot.effort, id.into(), id);
                    }
                });
        } else {
            design::mini_pill(ui, None, "Глубина: в модели AGY", p.muted, p);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn agent_editor(
    ui: &mut egui::Ui,
    a: &mut Agent,
    servers: &[Server],
    catalogs: &HashMap<String, ServerCatalogSnapshot>,
    cluster_mode: &str,
    worker_idx: Option<usize>,
    worker_count: usize,
    leader: bool,
    light: bool,
    p: Palette,
    refresh_requests: &mut Vec<(String, String)>,
) -> Option<WorkerCardAction> {
    let mut card_action = None;
    let srv = servers.iter().find(|s| s.id == a.server);
    let host_backend = srv.map(|s| s.effective_backend()).unwrap_or("claude");
    let effective_backend = if crate::model::valid_backend(&a.backend) {
        a.backend.clone()
    } else {
        host_backend.to_owned()
    };
    let b_col = design::backend_color(&effective_backend, light);
    let cat_snap = catalogs.get(&a.server);
    let online = cat_snap.is_some_and(|c| c.connected);
    let live_models: &[crate::model::ModelInfo] = cat_snap
        .map(|c| c.for_backend(&effective_backend))
        .unwrap_or(&[]);

    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(
            1.0_f32,
            if leader {
                design::mix(p.line, b_col, 0.45)
            } else {
                design::mix(p.line, b_col, 0.18)
            },
        ))
        .corner_radius(12.)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Header row: role badge, online indicator, worker controls
            ui.horizontal_wrapped(|ui| {
                let role_title = if leader {
                    "Руководитель (оркестратор)".into()
                } else {
                    format!("Исполнитель #{}", worker_idx.unwrap_or(0) + 1)
                };
                label(ui, role_title, 13.5, if leader { b_col } else { p.text });
                design::mini_pill(
                    ui,
                    Some(design::backend_icon(&effective_backend)),
                    crate::model::backend_short_label(&effective_backend),
                    b_col,
                    p,
                );
                if srv.is_some() {
                    design::mini_pill(
                        ui,
                        None,
                        if online { "● онлайн" } else { "○ офлайн" },
                        if online { GREEN } else { p.muted },
                        p,
                    );
                }
                if !leader {
                    ui.checkbox(&mut a.enabled, "Участвует");
                    if let Some(idx) = worker_idx {
                        if idx > 0 && ui.small_button("↑").on_hover_text("Поднять выше").clicked() {
                            card_action = Some(WorkerCardAction::MoveUp(idx));
                        }
                        if idx + 1 < worker_count
                            && ui.small_button("↓").on_hover_text("Опустить ниже").clicked()
                        {
                            card_action = Some(WorkerCardAction::MoveDown(idx));
                        }
                        if worker_count < 12
                            && ui.small_button("Копия").on_hover_text("Дублировать исполнителя").clicked()
                        {
                            card_action = Some(WorkerCardAction::Duplicate(idx));
                        }
                        if ui.small_button("Убрать").clicked() {
                            card_action = Some(WorkerCardAction::Remove(idx));
                        }
                    }
                }
            });

            ui.add_space(6.);
            // Name + Host + CWD row
            let prev_server = a.server.clone();
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut a.name)
                        .desired_width(185.)
                        .hint_text("Имя агента"),
                );
                egui::ComboBox::from_id_salt(("agent-server", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(
                        srv.map(|s| format!("{} {}", if online { "●" } else { "○" }, s.name))
                            .unwrap_or_else(|| "Выбрать хост".into()),
                    )
                    .show_ui(ui, |ui| {
                        for s in servers {
                            let s_on = catalogs.get(&s.id).is_some_and(|c| c.connected);
                            let txt = format!(
                                "{} {} ({})",
                                if s_on { "●" } else { "○" },
                                s.name,
                                crate::model::backend_short_label(s.effective_backend())
                            );
                            ui.selectable_value(&mut a.server, s.id.clone(), txt);
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut a.cwd)
                        .desired_width((ui.available_width() - 4.).max(180.))
                        .hint_text("Рабочая папка · пусто — папка хоста"),
                );
            });
            if a.server != prev_server && !a.server.is_empty() {
                let next_srv = servers.iter().find(|s| s.id == a.server);
                let next_eff = if crate::model::valid_backend(&a.backend) {
                    a.backend.as_str()
                } else {
                    next_srv.map(|s| s.effective_backend()).unwrap_or("claude")
                };
                if a.backend.is_empty() {
                    a.model = "inherit".into();
                }
                refresh_requests.push((a.server.clone(), next_eff.to_owned()));
            }

            ui.add_space(6.);
            // Engine + Live Model + Effort + Mode override row
            let srv = servers.iter().find(|s| s.id == a.server);
            let host_backend = srv.map(|s| s.effective_backend()).unwrap_or("claude");
            let prev_backend = a.backend.clone();
            ui.horizontal_wrapped(|ui| {
                let backend_text = match a.backend.as_str() {
                    "claude" => "Движок: Claude Code".into(),
                    "codex" => "Движок: Codex".into(),
                    "agy" => "Движок: AGY".into(),
                    "api" => "Движок: API".into(),
                    _ => format!("Движок: по хосту ({})", crate::model::backend_short_label(host_backend)),
                };
                egui::ComboBox::from_id_salt(("agent-backend", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(backend_text)
                    .show_ui(ui, |ui| {
                        let host_opt = format!(
                            "По профилю хоста ({})",
                            crate::model::backend_short_label(host_backend)
                        );
                        ui.selectable_value(&mut a.backend, String::new(), host_opt);
                        for (id, name) in [
                            ("agy", "Antigravity (AGY)"),
                            ("claude", "Claude Code"),
                            ("codex", "Codex"),
                            ("api", "Пользовательский API"),
                        ] {
                            ui.selectable_value(&mut a.backend, id.into(), name);
                        }
                    });
                let eff_backend = if crate::model::valid_backend(&a.backend) {
                    a.backend.as_str()
                } else {
                    host_backend
                };
                if a.backend != prev_backend {
                    a.model = "inherit".into();
                    a.effort = if eff_backend == "agy" {
                        "default".into()
                    } else {
                        "inherit".into()
                    };
                    if !a.server.is_empty() {
                        refresh_requests.push((a.server.clone(), eff_backend.to_owned()));
                    }
                }

                let models = catalogs
                    .get(&a.server)
                    .map(|c| c.for_backend(eff_backend))
                    .unwrap_or(&[]);
                let host_model = srv.map(|s| s.model_for(eff_backend)).unwrap_or("default");
                let model_btn = format!("Модель: {}", format_model_choice(models, &a.model, host_model));
                egui::ComboBox::from_id_salt(("agent-model", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(model_btn)
                    .show_ui(ui, |ui| {
                        let inherit_label = format_model_choice(models, "inherit", host_model);
                        ui.selectable_value(&mut a.model, "inherit".into(), inherit_label)
                            .on_hover_text("Взять модель, выбранную в профиле этого сервера");
                        ui.selectable_value(
                            &mut a.model,
                            "default".into(),
                            "Не задавать (авто движка)",
                        )
                        .on_hover_text("Не передавать --model: движок сам выберет свою модель по умолчанию");
                        ui.separator();
                        if !models.is_empty() {
                            egui::ScrollArea::vertical()
                                .max_height(240.)
                                .show(ui, |ui| {
                                    for m in models.iter().filter(|m| crate::model::explicit_model(&m.value)) {
                                        let disp = super::model_label(models, &m.value);
                                        let tip = if m.description.is_empty() {
                                            m.value.clone()
                                        } else {
                                            format!("{}\n{}", m.value, m.description)
                                        };
                                        ui.selectable_value(&mut a.model, m.value.clone(), disp)
                                            .on_hover_text(tip);
                                    }
                                });
                        } else if eff_backend == "claude" {
                            for (id, name) in [("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")] {
                                ui.selectable_value(&mut a.model, id.into(), name);
                            }
                        } else if eff_backend == "api" {
                            label(ui, "Список API-моделей пуст — проверь настройки API на сервере или введи ID ниже", 11., p.muted);
                        } else {
                            for m in crate::model::default_backend_models(eff_backend) {
                                ui.selectable_value(&mut a.model, m.value, m.display_name);
                            }
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            label(ui, "Своя:", 11.5, p.muted);
                            ui.add(
                                egui::TextEdit::singleline(&mut a.model)
                                    .desired_width(185.)
                                    .hint_text("ID модели (или default / inherit)"),
                            );
                        });
                    });

                if online && matches!(eff_backend, "claude" | "codex" | "agy") {
                    if ui
                        .small_button("↻")
                        .on_hover_text("Обновить список моделей с сервера")
                        .clicked()
                    {
                        refresh_requests.push((a.server.clone(), eff_backend.to_owned()));
                    }
                }

                // Effort control (only for engines where effort is separate from model)
                if crate::model::backend_supports_effort(eff_backend) {
                    let host_effort = srv.map(|s| s.effort.as_str()).unwrap_or("default");
                    egui::ComboBox::from_id_salt(("agent-effort", &a.id))
                        .icon(design::combo_icon)
                        .selected_text(format!("Глубина: {}", effort_display(&a.effort, host_effort)))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut a.effort,
                                "inherit".into(),
                                effort_display("inherit", host_effort),
                            );
                            ui.selectable_value(&mut a.effort, "default".into(), "По умолчанию движка");
                            ui.separator();
                            for id in effort_choices(eff_backend, &a.model, host_model, models) {
                                ui.selectable_value(&mut a.effort, id.into(), id);
                            }
                        });
                } else {
                    design::mini_pill(ui, None, "Глубина: в модели AGY", p.muted, p)
                        .on_hover_text("У AGY глубина рассуждений (Low / Medium / High) выбирается самой моделью");
                }

                // Per-agent permission mode override
                let mode_btn = if a.mode.is_empty() || a.mode == "inherit" {
                    format!("Права: команды ({})", mode_label(cluster_mode))
                } else {
                    format!("Права: {}", mode_label(&a.mode))
                };
                egui::ComboBox::from_id_salt(("agent-mode", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(mode_btn)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut a.mode,
                            String::new(),
                            format!("Как у команды ({})", mode_label(cluster_mode)),
                        );
                        ui.separator();
                        for id in ["auto", "acceptEdits", "plan", "manual"] {
                            ui.selectable_value(&mut a.mode, id.into(), mode_label(id));
                        }
                    });
            });

            // Effective resolved run summary so user has 100% clarity
            if let Some(s) = srv {
                let eff_b = super::cluster_runtime::agent_backend(a, s);
                let eff_m = super::cluster_runtime::agent_model(a, s);
                let eff_m_str = if eff_m == "default" {
                    "авто движка".into()
                } else {
                    super::model_label(live_models, &eff_m)
                };
                let eff_e = super::cluster_runtime::agent_effort(a, s);
                let eff_mode = super::cluster_runtime::agent_mode(a, cluster_mode);
                let eff_cwd = super::cluster_runtime::agent_cwd(a, s);
                let effort_part = if eff_b == "agy" || eff_e == "default" {
                    String::new()
                } else {
                    format!(" · глубина: {eff_e}")
                };
                ui.add_space(4.);
                label(
                    ui,
                    format!(
                        "Запуск: {} · {} · {}{} · {} · {}",
                        s.name,
                        crate::model::backend_short_label(eff_b),
                        eff_m_str,
                        effort_part,
                        mode_label(&eff_mode),
                        model::shorten(&eff_cwd, 36),
                    ),
                    11.,
                    p.muted,
                );
            }
        });
    card_action
}
fn variable_picker(
    ui: &mut egui::Ui,
    text: &mut String,
    field: egui::Id,
    vars: &[Variable],
    p: Palette,
) {
    egui::ComboBox::from_id_salt(field.with("variables"))
        .icon(design::combo_icon)
        .selected_text(RichText::new("Вставить ключ").size(12.).color(p.muted))
        .show_ui(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(250.)
                .show(ui, |ui| {
                    for v in vars.iter().filter(|v| !v.label.ends_with(" · ID")) {
                        if ui
                            .selectable_label(false, format!("{}  {}", v.token, v.label))
                            .on_hover_text(&v.value)
                            .clicked()
                        {
                            let pos = egui::TextEdit::load_state(ui.ctx(), field)
                                .and_then(|s| s.cursor.char_range())
                                .map(|r| r.primary.index)
                                .unwrap_or_else(|| text.chars().count());
                            let at = text
                                .char_indices()
                                .nth(pos)
                                .map(|(i, _)| i)
                                .unwrap_or(text.len());
                            text.insert_str(at, &v.token);
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field) {
                                state
                                    .cursor
                                    .set_char_range(Some(egui::text::CCursorRange::one(
                                        egui::text::CCursor::new(pos + v.token.chars().count()),
                                    )));
                                state.store(ui.ctx(), field);
                            }
                            ui.memory_mut(|m| m.request_focus(field));
                        }
                    }
                });
        });
}
struct NodeTelemetry {
    id: String,
    name: String,
    host: String,
    backend: String,
    model_tag: String,
    status: String,
    detail: String,
    tel: crate::model::TurnTelemetry,
    active: bool,
    done: bool,
    delivered: bool,
    failed: bool,
    assigned_count: usize,
    done_count: usize,
}

fn build_node_telemetry(
    app: &RelayApp,
    run: Option<&Run>,
    agent: &Agent,
    cluster_mode: &str,
    leader: bool,
    _light: bool,
) -> NodeTelemetry {
    let srv = app.settings.servers.iter().find(|s| s.id == agent.server);
    let host = srv
        .map(|s| s.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "хост".into());
    let backend = if crate::model::valid_backend(&agent.backend) {
        agent.backend.clone()
    } else {
        srv.map(|s| s.effective_backend().to_owned())
            .unwrap_or_else(|| "claude".into())
    };
    let model_name = agent_resolved_model_display(app, agent, srv, &backend);
    let eff_effort = srv
        .map(|s| super::cluster_runtime::agent_effort(agent, s))
        .unwrap_or_else(|| agent.effort.clone());
    let eff_mode = super::cluster_runtime::agent_mode(agent, cluster_mode);
    let mut model_tag = model_name;
    if backend != "agy" && eff_effort != "default" && !eff_effort.is_empty() {
        model_tag.push_str(&format!(" · {eff_effort}"));
    }
    if eff_mode != cluster_mode {
        model_tag.push_str(&format!(" · {}", mode_label(&eff_mode)));
    }

    if leader {
        let (status, active, sid_opt) = if let Some(r) = run {
            let k = key(&r.config.leader.server, &r.leader_sid);
            let st = if r.paused {
                ("На паузе".into(), false)
            } else {
                match app.chats.get(&k) {
                    Some(chat) if !chat.permissions.is_empty() => {
                        ("Ждёт подтверждения".into(), true)
                    }
                    Some(chat) if chat.busy => ("Координирует команду".into(), true),
                    _ if r.tasks.iter().any(|t| !t.state.terminal()) => {
                        ("Принимает отчёты".into(), true)
                    }
                    _ => ("Ожидает цель".into(), false),
                }
            };
            (st.0, st.1, Some(k))
        } else {
            ("Готов к запуску".into(), false, None)
        };
        let tel = sid_opt
            .as_ref()
            .and_then(|k| app.chats.get(k))
            .map(|c| crate::model::analyze_turn(&c.messages, c.user_at, c.busy, &c.status))
            .unwrap_or_default();
        let detail = if !tel.active_target.is_empty() {
            tel.active_target.clone()
        } else if let Some(r) = run {
            model::shorten(&r.title, 42)
        } else {
            model_tag.clone()
        };
        let assigned_count = run.map(|r| r.tasks.len()).unwrap_or(0);
        let done_count = run
            .map(|r| r.tasks.iter().filter(|t| t.state == TaskState::Done).count())
            .unwrap_or(0);
        return NodeTelemetry {
            id: "leader".into(),
            name: agent.name.clone(),
            host,
            backend,
            model_tag,
            status,
            detail,
            tel,
            active,
            done: assigned_count > 0 && done_count == assigned_count && !active,
            delivered: false,
            failed: false,
            assigned_count,
            done_count,
        };
    }

    let worker_tasks: Vec<_> = run
        .map(|r| r.tasks.iter().filter(|t| t.agent.id == agent.id).collect())
        .unwrap_or_default();
    let assigned_count = worker_tasks.len();
    let done_count = worker_tasks
        .iter()
        .filter(|t| t.state == TaskState::Done)
        .count();
    let latest = worker_tasks.last().copied();
    let status = if let Some(t) = latest {
        t.state.label().to_owned()
    } else if app.connected(&agent.server) {
        "Готов".into()
    } else {
        "Нет связи".into()
    };
    let active = latest.is_some_and(|t| matches!(t.state, TaskState::Running | TaskState::Waiting));
    let done = latest.is_some_and(|t| t.state == TaskState::Done);
    let delivered = latest.is_some_and(|t| t.delivered);
    let failed =
        latest.is_some_and(|t| matches!(t.state, TaskState::Failed | TaskState::Interrupted));

    let tel = latest
        .and_then(|t| app.chats.get(&key(&t.agent.server, &t.sid)))
        .map(|c| crate::model::analyze_turn(&c.messages, c.user_at, c.busy, &c.status))
        .unwrap_or_default();
    let detail = if active && !tel.active_target.is_empty() {
        tel.active_target.clone()
    } else if active && !tel.active_headline.is_empty() {
        tel.active_headline.clone()
    } else if let Some(t) = latest {
        model::shorten(&t.title, 42)
    } else {
        model_tag.clone()
    };

    NodeTelemetry {
        id: agent.id.clone(),
        name: agent.name.clone(),
        host,
        backend,
        model_tag,
        status,
        detail,
        tel,
        active,
        done,
        delivered,
        failed,
        assigned_count,
        done_count,
    }
}

fn state_color(state: TaskState, p: Palette) -> Color32 {
    match state {
        TaskState::Done => GREEN,
        TaskState::Running | TaskState::Waiting => accent(),
        TaskState::Failed | TaskState::Interrupted => Color32::from_rgb(228, 126, 136),
        _ => p.muted,
    }
}

fn bezier_point(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, p3: egui::Pos2, t: f32) -> egui::Pos2 {
    let q = 1.0 - t;
    (p0.to_vec2() * (q * q * q)
        + p1.to_vec2() * (3.0 * q * q * t)
        + p2.to_vec2() * (3.0 * q * t * t)
        + p3.to_vec2() * (t * t * t))
    .to_pos2()
}

fn draw_graph(
    ui: &mut egui::Ui,
    leader: &NodeTelemetry,
    workers: &[NodeTelemetry],
    light: bool,
    p: Palette,
) -> Option<String> {
    let width = ui.available_width();
    let narrow = width < 640.;
    let node_h = 104.;
    let row_step = 116.;
    let height = if narrow {
        166. + workers.len() as f32 * row_step
    } else {
        (workers.len() as f32 * row_step + 34.).max(264.)
    };
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    ui.painter()
        .rect_filled(area, 16., design::mix(p.side, p.base, 0.38));
    ui.painter().rect_stroke(
        area,
        16.,
        Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.22)),
        egui::StrokeKind::Inside,
    );
    design::specular_rim(ui.painter(), area, 16., light);

    // Subtle next-gen coordinate dot-matrix canvas
    let grid_step = 24.0_f32;
    let dot_col = p.line.gamma_multiply(if light { 0.55 } else { 0.42 });
    let mut gx = area.left() + 18.0;
    while gx < area.right() - 12.0 {
        let mut gy = area.top() + 18.0;
        while gy < area.bottom() - 12.0 {
            ui.painter().circle_filled(egui::pos2(gx, gy), 0.95, dot_col);
            gy += grid_step;
        }
        gx += grid_step;
    }

    let w = if narrow {
        (width - 40.).min(320.)
    } else {
        (width * 0.38).clamp(186., 296.)
    };
    let leader_rect = egui::Rect::from_center_size(
        egui::pos2(
            if narrow {
                area.center().x
            } else {
                area.left() + w / 2. + 24.
            },
            if narrow {
                area.top() + 68.
            } else {
                area.center().y
            },
        ),
        Vec2::new(w, node_h + 6.),
    );

    // Ambient radial aura behind leader node
    let lead_col = design::backend_color(&leader.backend, light);
    ui.painter().circle_filled(
        leader_rect.center(),
        w * 0.44,
        lead_col.gamma_multiply(if leader.active { 0.08 } else { 0.04 }),
    );

    let animated = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("relay-animations")))
        .unwrap_or(true);
    let now = ui.input(|i| i.time) as f32;
    let mut any_anim = false;
    let mut clicked = None;

    let total_workers_h = if workers.is_empty() {
        0.
    } else {
        (workers.len() - 1) as f32 * row_step
    };
    let workers_start_y = if narrow {
        area.top() + 190.
    } else {
        area.center().y - total_workers_h * 0.5
    };

    for (i, w_node) in workers.iter().enumerate() {
        let node = egui::Rect::from_center_size(
            egui::pos2(
                if narrow {
                    area.center().x
                } else {
                    area.right() - w / 2. - 24.
                },
                workers_start_y + i as f32 * row_step,
            ),
            Vec2::new(w, node_h),
        );
        let w_col = design::backend_color(&w_node.backend, light);
        if w_node.active {
            ui.painter()
                .circle_filled(node.center(), w * 0.38, w_col.gamma_multiply(0.075));
        }

        let start = if narrow {
            leader_rect.center_bottom()
        } else {
            leader_rect.right_center()
        };
        let end = if narrow {
            node.center_top()
        } else {
            node.left_center()
        };
        let d = if narrow {
            Vec2::new(0., (end.y - start.y) * 0.52)
        } else {
            Vec2::new((end.x - start.x) * 0.52, 0.)
        };
        let p1 = start + d;
        let p2 = end - d;
        let line_col = if w_node.failed {
            Color32::from_rgb(228, 126, 136)
        } else if w_node.active {
            w_col
        } else if w_node.done {
            GREEN.gamma_multiply(0.75)
        } else {
            p.line
        };

        // Soft outer conduit glow + crisp inner conduit
        if w_node.active || w_node.done {
            ui.painter()
                .add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [start, p1, p2, end],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(5.5_f32, line_col.gamma_multiply(0.14)),
                ));
        }
        ui.painter()
            .add(egui::epaint::CubicBezierShape::from_points_stroke(
                [start, p1, p2, end],
                false,
                Color32::TRANSPARENT,
                Stroke::new(
                    if w_node.active { 2.0_f32 } else { 1.25_f32 },
                    line_col,
                ),
            ));
        // Endpoint anchor sockets
        ui.painter().circle_filled(start, 3.0, lead_col);
        ui.painter().circle_filled(end, 3.0, line_col);

        // Multi-particle forward stream while active; return telemetry pulse when delivered
        if w_node.active {
            any_anim = true;
            for k in 0..3 {
                let t = if animated {
                    (now * 0.48 + k as f32 * 0.33 + i as f32 * 0.17) % 1.0
                } else {
                    0.25 + k as f32 * 0.28
                };
                let pt = bezier_point(start, p1, p2, end, t);
                ui.painter()
                    .circle_filled(pt, 5.2, w_col.gamma_multiply(0.22));
                ui.painter().circle_filled(pt, 2.7, w_col);
            }
        } else if w_node.delivered {
            let t = if animated {
                any_anim = true;
                1.0 - ((now * 0.30 + i as f32 * 0.25) % 1.0)
            } else {
                0.5
            };
            let pt = bezier_point(start, p1, p2, end, t);
            ui.painter()
                .circle_filled(pt, 4.6, GREEN.gamma_multiply(0.22));
            ui.painter().circle_filled(pt, 2.3, GREEN);
        }

        if graph_node(ui, node, w_node, false, light, p).clicked() {
            clicked = Some(w_node.id.clone());
        }
    }

    if graph_node(ui, leader_rect, leader, true, light, p).clicked() {
        clicked = Some("leader".into());
    }
    if any_anim && animated {
        ui.ctx()
            .request_repaint_after(Duration::from_millis(28));
    }
    clicked
}

fn graph_node(
    ui: &mut egui::Ui,
    r: egui::Rect,
    n: &NodeTelemetry,
    leader: bool,
    light: bool,
    p: Palette,
) -> egui::Response {
    let response = ui.interact(r, ui.id().with(("agent-graph", &n.id)), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        design::duration(ui.ctx(), 0.18),
    );
    let b_col = design::backend_color(&n.backend, light);
    let status_col = if n.failed {
        Color32::from_rgb(228, 126, 136)
    } else if n.active {
        b_col
    } else if n.done {
        GREEN
    } else {
        p.muted
    };

    ui.painter().rect_filled(
        r,
        13.,
        design::mix(
            p.surface,
            design::mix(p.line, b_col, 0.25),
            if n.active { 0.22 } else { 0.06 } + hover * 0.28,
        ),
    );
    ui.painter().rect_stroke(
        r,
        13.,
        Stroke::new(
            if leader || n.active { 1.45_f32 } else { 1.0_f32 },
            if n.active || leader {
                design::mix(p.line, b_col, 0.62)
            } else {
                design::mix(p.line, b_col, 0.22 + hover * 0.35)
            },
        ),
        egui::StrokeKind::Inside,
    );
    // Signature top-left accent bar
    ui.painter().line_segment(
        [
            egui::pos2(r.left() + 14., r.top() + 1.0),
            egui::pos2(r.right() - 14., r.top() + 1.0),
        ],
        Stroke::new(1.6_f32, b_col.gamma_multiply(if n.active || leader { 0.65 } else { 0.28 })),
    );

    // Orbital / status emblem in top-right of card
    let orb_rect = egui::Rect::from_center_size(
        egui::pos2(r.right() - 22., r.top() + 22.),
        Vec2::splat(20.),
    );
    if n.active {
        design::orbital_core(ui, orb_rect, b_col, true);
    } else if n.done {
        ui.painter()
            .circle_filled(orb_rect.center(), 9.5, GREEN.gamma_multiply(0.16));
        design::icon(ui.painter(), orb_rect.shrink(4.), Icon::Check, GREEN);
    } else {
        ui.painter()
            .circle_filled(orb_rect.center(), 9.5, b_col.gamma_multiply(0.13));
        design::icon(
            ui.painter(),
            orb_rect.shrink(4.),
            design::backend_icon(&n.backend),
            b_col,
        );
    }

    let painter = ui.painter().with_clip_rect(r.shrink2(Vec2::new(12., 8.)));
    let role_tag = format!(
        "{} · {} · @{}",
        if leader { "ОРКЕСТРАТОР" } else { "ИСПОЛНИТЕЛЬ" },
        crate::model::backend_short_label(&n.backend).to_uppercase(),
        model::shorten(&n.host, 12),
    );
    painter.text(
        r.left_top() + Vec2::new(13., 11.),
        egui::Align2::LEFT_TOP,
        role_tag,
        FontId::proportional(9.5),
        b_col,
    );
    painter.text(
        r.left_top() + Vec2::new(13., 28.),
        egui::Align2::LEFT_TOP,
        model::shorten(&n.name, 24),
        FontId::proportional(14.5),
        p.text,
    );
    painter.text(
        r.left_top() + Vec2::new(13., 49.),
        egui::Align2::LEFT_TOP,
        model::shorten(&n.detail, 34),
        FontId::monospace(10.8),
        design::mix(p.text, p.muted, 0.35),
    );

    // Bottom telemetry row: status + mini phase bar or task counter
    painter.circle_filled(r.left_bottom() + Vec2::new(17., -15.), 3.0, status_col);
    painter.text(
        r.left_bottom() + Vec2::new(24., -9.),
        egui::Align2::LEFT_BOTTOM,
        &n.status,
        FontId::proportional(10.8),
        status_col,
    );
    if n.tel.total_actions > 0 && r.width() >= 210. {
        let pb = egui::Rect::from_min_size(
            egui::pos2(r.right() - 62., r.bottom() - 18.),
            Vec2::new(48., 5.),
        );
        design::phase_bar(ui.painter(), pb, &n.tel, light, p);
    } else if n.assigned_count > 0 && r.width() >= 210. {
        painter.text(
            r.right_bottom() + Vec2::new(-13., -9.),
            egui::Align2::RIGHT_BOTTOM,
            format!("{}/{}", n.done_count, n.assigned_count),
            FontId::proportional(10.5),
            p.muted,
        );
    }

    response
        .on_hover_text(format!(
            "{} · {} · {}",
            n.name,
            crate::model::backend_short_label(&n.backend),
            n.model_tag
        ))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
