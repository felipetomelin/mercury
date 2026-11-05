//! Guided cron editor. Persistence and occurrence previews belong to the server.
use super::*;
use crate::api::schema::{CronJobDefinition, CronPreviewParams, Method};
use mercury_cron::JobTarget;
use ratatui::{
    style::Modifier,
    widgets::{Clear, Widget},
};

const PRESETS: &[&str] = &[
    "Dias úteis",
    "Diariamente",
    "Semanalmente",
    "Mensalmente",
    "A cada intervalo",
    "Daqui a… (uma vez)",
    "Data e hora (uma vez)",
    "Avançado · expressão",
];
const TARGETS: &[&str] = &[
    "Claude · segundo plano",
    "Codex · segundo plano",
    "Painel existente",
    "Programa local",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Name,
    Preset,
    Number,
    Unit,
    Date,
    Time,
    Days,
    MonthDay,
    Raw,
    Offset,
    Kind,
    Address,
    Args,
    Prompt,
    DirectoryMode,
    Directory,
    Timeout,
    Enabled,
    Review,
    Cancel,
}

#[derive(Debug)]
pub(super) struct CronForm {
    generation: String,
    pub job_id: Option<String>,
    original: Option<CronJobDefinition>,
    pub inputs: Vec<(Field, TextEditor)>,
    pub focus: usize,
    pub preset: usize,
    pub kind: usize,
    unit: usize,
    directory_mode: usize,
    workspace_dir: String,
    pub days: [bool; 7],
    day_cursor: usize,
    pub enabled: bool,
    pub reviewing: bool,
    pub submitting: bool,
    pub error: Option<String>,
    pub preview: Vec<i64>,
    pub preview_key: Option<String>,
    preview_requested: Option<String>,
    pub scroll: usize,
    pub panes: Vec<(String, String)>,
}

impl CronForm {
    pub fn new(
        workspace_dir: String,
        panes: Vec<(String, String)>,
        edit: Option<(String, CronJobDefinition)>,
    ) -> Self {
        let mut form = Self {
            generation: format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            job_id: None,
            original: None,
            inputs: Vec::new(),
            focus: 0,
            preset: 0,
            kind: 1,
            unit: 0,
            directory_mode: 0,
            workspace_dir,
            days: [false, true, true, true, true, true, false],
            day_cursor: 1,
            enabled: true,
            reviewing: false,
            submitting: false,
            error: None,
            preview: Vec::new(),
            preview_key: None,
            preview_requested: None,
            scroll: 0,
            panes,
        };
        for (field, value) in [
            (Field::Name, ""),
            (Field::Number, "30"),
            (Field::Date, ""),
            (Field::Time, "09:00"),
            (Field::MonthDay, "1"),
            (Field::Raw, "0 9 * * 1-5"),
            (Field::Offset, ""),
            (Field::Address, ""),
            (Field::Args, ""),
            (Field::Prompt, ""),
            (Field::Directory, ""),
            (Field::Timeout, "300"),
        ] {
            let editor = if matches!(field, Field::Prompt | Field::Args) {
                TextEditor::multiline(value)
            } else {
                TextEditor::new(value, true)
            };
            form.inputs.push((field, editor));
        }
        if let Some((id, definition)) = edit {
            form.job_id = Some(id);
            form.preset = 7;
            form.set(Field::Name, &definition.name);
            form.set(Field::Raw, &definition.schedule);
            form.set(
                Field::Offset,
                definition.utc_offset.as_deref().unwrap_or(""),
            );
            form.set(
                Field::Directory,
                &definition
                    .workdir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            );
            form.directory_mode = 1;
            form.set(
                Field::Timeout,
                &definition
                    .timeout_seconds
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            );
            form.enabled = definition.enabled.unwrap_or(true);
            match &definition.target {
                JobTarget::Headless {
                    provider, prompt, ..
                } => {
                    form.kind = usize::from(provider == "codex");
                    form.set(Field::Prompt, prompt);
                }
                JobTarget::Pane { pane_id, prompt } => {
                    form.kind = 2;
                    form.set(Field::Address, pane_id);
                    form.set(Field::Prompt, prompt);
                }
                JobTarget::Script { program, args } => {
                    form.kind = 3;
                    form.set(Field::Address, program);
                    form.set(Field::Args, &args.join("\n"));
                }
            }
            form.original = Some(definition);
        }
        form
    }
    pub fn value(&self, field: Field) -> &str {
        self.inputs
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, e)| e.as_str())
            .unwrap_or("")
    }
    pub(super) fn set(&mut self, field: Field, value: &str) {
        if let Some((_, e)) = self.inputs.iter_mut().find(|(f, _)| *f == field) {
            *e = if matches!(field, Field::Prompt | Field::Args) {
                TextEditor::multiline(value)
            } else {
                TextEditor::new(value, true)
            };
        }
    }
    pub fn fields(&self) -> Vec<Field> {
        if self.reviewing {
            return vec![Field::Review, Field::Cancel];
        }
        let mut fields = vec![Field::Name, Field::Preset];
        match self.preset {
            0 | 1 => fields.push(Field::Time),
            2 => fields.extend([Field::Days, Field::Time]),
            3 => fields.extend([Field::MonthDay, Field::Time]),
            4 | 5 => fields.extend([Field::Number, Field::Unit]),
            6 => fields.extend([Field::Date, Field::Time]),
            _ => fields.push(Field::Raw),
        }
        fields.extend([Field::Offset, Field::Kind]);
        if self.kind >= 2 {
            fields.push(Field::Address);
        }
        fields.push(if self.kind == 3 {
            Field::Args
        } else {
            Field::Prompt
        });
        fields.push(Field::DirectoryMode);
        if self.directory_mode == 1 {
            fields.push(Field::Directory);
        }
        fields.extend([Field::Timeout, Field::Enabled, Field::Review, Field::Cancel]);
        fields
    }
    pub fn focused(&self) -> Field {
        let f = self.fields();
        f[self.focus.min(f.len() - 1)]
    }
    pub fn insert(&mut self, text: &str) {
        let field = self.focused();
        if text.len() <= 32768 && !self.reviewing && !self.submitting {
            if let Some((_, e)) = self.inputs.iter_mut().find(|(f, _)| *f == field) {
                if e.len() + text.len() <= 32768 {
                    e.insert(text);
                }
            }
            self.preview_key = None;
        }
    }
    pub fn schedule(&self) -> Result<String, String> {
        let number = |field, min, max| -> Result<u32, String> {
            self.value(field)
                .parse::<u32>()
                .ok()
                .filter(|n| *n >= min && *n <= max)
                .ok_or_else(|| format!("Valor deve estar entre {min} e {max}"))
        };
        let clock = || -> Result<(u32, u32), String> {
            let Some((h, m)) = self.value(Field::Time).split_once(':') else {
                return Err("Horário deve ser HH:MM".into());
            };
            let h = h
                .parse::<u32>()
                .ok()
                .filter(|h| *h < 24)
                .ok_or("Hora inválida")?;
            let m = m
                .parse::<u32>()
                .ok()
                .filter(|m| *m < 60)
                .ok_or("Minuto inválido")?;
            Ok((h, m))
        };
        Ok(match self.preset {
            0 | 1 => {
                let (h, m) = clock()?;
                format!("{m} {h} * * {}", if self.preset == 0 { "1-5" } else { "*" })
            }
            2 => {
                let (h, m) = clock()?;
                let days = self
                    .days
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| **b)
                    .map(|(i, _)| i.to_string())
                    .collect::<Vec<_>>();
                if days.is_empty() {
                    return Err("Selecione ao menos um dia".into());
                }
                format!("{m} {h} * * {}", days.join(","))
            }
            3 => {
                let (h, m) = clock()?;
                let d = number(Field::MonthDay, 1, 31)?;
                format!("{m} {h} {d} * *")
            }
            4 | 5 => {
                let n = number(Field::Number, 1, 100000)?;
                format!(
                    "{}{n}{}",
                    if self.preset == 4 { "every " } else { "" },
                    if self.unit == 0 { "m" } else { "h" }
                )
            }
            6 => {
                clock()?;
                let offset = self.value(Field::Offset);
                if offset.is_empty() {
                    return Err("Aguarde a consulta do fuso do servidor".into());
                }
                format!(
                    "{}T{}:00{}",
                    self.value(Field::Date),
                    self.value(Field::Time),
                    offset
                )
            }
            _ => self.value(Field::Raw).trim().to_string(),
        })
    }
    pub fn key(&self) -> Result<String, String> {
        Ok(format!(
            "{}|{}|{}",
            self.generation,
            self.schedule()?,
            self.value(Field::Offset)
        ))
    }
    pub fn definition(&self) -> Result<CronJobDefinition, String> {
        let name = self.value(Field::Name).trim();
        if name.is_empty() || name.len() > 120 {
            return Err("Nome deve ter 1–120 bytes".into());
        }
        let schedule = self.schedule()?;
        let offset = self.value(Field::Offset);
        if offset.is_empty() {
            return Err("Aguarde a consulta do fuso ou informe +HH:MM/-HH:MM".into());
        }
        mercury_cron::job::parse_utc_offset(offset)?;
        let prompt = self.value(Field::Prompt).trim().to_string();
        if self.kind != 3 && prompt.is_empty() {
            return Err("Escreva a instrução da tarefa".into());
        }
        let address = self.value(Field::Address).trim().to_string();
        let target = match self.kind {
            0 | 1 => {
                let provider = if self.kind == 0 { "claude" } else { "codex" }.to_string();
                if let Some(CronJobDefinition {
                    target:
                        JobTarget::Headless {
                            provider: old,
                            prompt_files,
                            model,
                            effort,
                            skills,
                            ..
                        },
                    ..
                }) = &self.original
                {
                    if old == &provider {
                        JobTarget::Headless {
                            provider,
                            prompt,
                            prompt_files: prompt_files.clone(),
                            model: model.clone(),
                            effort: effort.clone(),
                            skills: skills.clone(),
                        }
                    } else {
                        JobTarget::Headless {
                            provider,
                            prompt,
                            prompt_files: vec![],
                            model: None,
                            effort: None,
                            skills: vec![],
                        }
                    }
                } else {
                    JobTarget::Headless {
                        provider,
                        prompt,
                        prompt_files: vec![],
                        model: None,
                        effort: None,
                        skills: vec![],
                    }
                }
            }
            2 => {
                if !self.panes.iter().any(|(id, _)| id == &address) {
                    return Err("Selecione um painel existente".into());
                }
                JobTarget::Pane {
                    pane_id: address,
                    prompt,
                }
            }
            _ => {
                if !std::path::Path::new(&address).is_absolute() {
                    return Err("Programa deve ter caminho absoluto".into());
                }
                let input = self.value(Field::Args);
                let args = if let Some(CronJobDefinition {
                    target: JobTarget::Script { args, .. },
                    ..
                }) = &self.original
                {
                    if args.join("\n") == input {
                        args.clone()
                    } else if input.is_empty() {
                        vec![]
                    } else {
                        input.split('\n').map(str::to_string).collect()
                    }
                } else if input.is_empty() {
                    vec![]
                } else {
                    input.split('\n').map(str::to_string).collect()
                };
                JobTarget::Script {
                    program: address,
                    args,
                }
            }
        };
        let directory = if self.directory_mode == 0 {
            self.workspace_dir.as_str()
        } else {
            self.value(Field::Directory).trim()
        };
        if !directory.is_empty() && !std::path::Path::new(directory).is_absolute() {
            return Err("Diretório deve ser absoluto".into());
        }
        let timeout = if self.value(Field::Timeout).trim().is_empty() {
            None
        } else {
            Some(
                self.value(Field::Timeout)
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("Timeout deve ser número positivo em segundos")?,
            )
        };
        let mut definition = self.original.clone().unwrap_or(CronJobDefinition {
            name: String::new(),
            schedule: String::new(),
            utc_offset: None,
            target: target.clone(),
            workdir: None,
            timeout_seconds: None,
            enabled: None,
            catch_up: mercury_cron::CatchUp::Skip,
            repeat_times: None,
            deliver: vec![mercury_cron::Delivery::Local { notify: true }],
        });
        definition.name = name.into();
        definition.schedule = schedule;
        definition.utc_offset = Some(offset.into());
        definition.target = target;
        definition.workdir = (!directory.is_empty()).then(|| directory.into());
        definition.timeout_seconds = timeout;
        definition.enabled = Some(self.enabled);
        Ok(definition)
    }
    fn choice(&mut self, field: Field, delta: isize) {
        fn shift(v: usize, n: usize, d: isize) -> usize {
            (v as isize + d).rem_euclid(n as isize) as usize
        }
        match field {
            Field::Preset => self.preset = shift(self.preset, PRESETS.len(), delta),
            Field::Kind => self.kind = shift(self.kind, TARGETS.len(), delta),
            Field::Unit => self.unit = shift(self.unit, 2, delta),
            Field::DirectoryMode => self.directory_mode = shift(self.directory_mode, 2, delta),
            Field::Enabled => self.enabled = !self.enabled,
            Field::Days => self.day_cursor = shift(self.day_cursor, 7, delta),
            Field::Address if self.kind == 2 && !self.panes.is_empty() => {
                let at = self
                    .panes
                    .iter()
                    .position(|(id, _)| id == self.value(Field::Address))
                    .unwrap_or(0);
                let id = self.panes[shift(at, self.panes.len(), delta)].0.clone();
                self.set(Field::Address, &id);
            }
            _ => {}
        }
        self.focus = self.focus.min(self.fields().len() - 1);
        self.preview_key = None;
    }
    fn label(&self, field: Field) -> &'static str {
        match field {
            Field::Name => "Nome",
            Field::Preset => "Agenda · ← → selecionar",
            Field::Number => "Quantidade",
            Field::Unit => "Unidade · ← →",
            Field::Date => "Data · AAAA-MM-DD",
            Field::Time => "Horário · HH:MM",
            Field::Days => "Dias · ← → mover, Espaço marcar",
            Field::MonthDay => "Dia do mês · 1 a 31",
            Field::Raw => "Expressão avançada · cron, every ou RFC3339",
            Field::Offset => "Fuso UTC fixo · +HH:MM/-HH:MM (sem ajuste de verão)",
            Field::Kind => "Executar com · ← → selecionar",
            Field::Address => {
                if self.kind == 2 {
                    "Painel · ← → selecionar"
                } else {
                    "Programa · caminho absoluto"
                }
            }
            Field::Args => "Argumentos · um por linha; Alt+Enter nova linha",
            Field::Prompt => "Instrução · Alt+Enter nova linha; texto longo é rolável",
            Field::DirectoryMode => "Diretório · ← → selecionar",
            Field::Directory => "Caminho absoluto",
            Field::Timeout => "Timeout em segundos · vazio usa padrão",
            Field::Enabled => "Estado inicial · ← → alterar",
            Field::Review => {
                if self.reviewing {
                    "Salvar agendamento"
                } else {
                    "Revisar agendamento"
                }
            }
            Field::Cancel => {
                if self.reviewing {
                    "Voltar ao formulário"
                } else {
                    "Cancelar"
                }
            }
        }
    }
    fn display(&self, field: Field) -> String {
        match field {
            Field::Preset => PRESETS[self.preset].into(),
            Field::Kind => TARGETS[self.kind].into(),
            Field::Unit => if self.unit == 0 { "minutos" } else { "horas" }.into(),
            Field::DirectoryMode => {
                if self.directory_mode == 0 {
                    format!("Workspace atual · {}", self.workspace_dir)
                } else {
                    "Caminho personalizado".into()
                }
            }
            Field::Enabled => if self.enabled { "Ativo" } else { "Pausado" }.into(),
            Field::Days => ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"]
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    format!(
                        "{}{}{}",
                        if i == self.day_cursor { ">" } else { " " },
                        if self.days[i] { "[x]" } else { "[ ]" },
                        d
                    )
                })
                .collect::<Vec<_>>()
                .join(" "),
            Field::Address if self.kind == 2 => self
                .panes
                .iter()
                .find(|(id, _)| id == self.value(field))
                .map(|(id, l)| format!("{l} · {id}"))
                .unwrap_or_else(|| "← → selecione um painel".into()),
            Field::Offset if self.value(field).is_empty() => "Consultando fuso do servidor…".into(),
            Field::Review | Field::Cancel => format!("[ {} ]", self.label(field)),
            _ => {
                if self.focused() == field && !self.reviewing {
                    self.inputs
                        .iter()
                        .find(|(f, _)| *f == field)
                        .map(|(_, e)| e.with_caret())
                        .unwrap_or_default()
                } else {
                    self.value(field).to_string()
                }
            }
        }
    }
}

pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    form: &mut CronForm,
    hits: &mut ShellHitMap,
) {
    use super::render::put_text;
    Clear.render(area, buffer);
    hits.crons_rows.clear();
    if area.width < 4 || area.height < 5 {
        return;
    }
    let p = &config.palette;
    buffer.set_style(area, Style::default().fg(p.text).bg(p.panel_bg));
    put_text(
        buffer,
        area.x + 1,
        area.y,
        area.width - 2,
        if form.reviewing {
            "Crons · revisar antes de salvar"
        } else if form.job_id.is_some() {
            "Crons · editar agendamento"
        } else {
            "Crons · novo agendamento"
        },
        Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
    );
    put_text(
        buffer,
        area.x + 1,
        area.y + 1,
        area.width - 2,
        "Tab/↑↓ campo · ← → opção · Enter avançar · Esc voltar/cancelar",
        Style::default().fg(p.subtext0),
    );
    let next_text = if form.preview_key.as_ref() == form.key().ok().as_ref() {
        form.preview
            .first()
            .and_then(|at| time::OffsetDateTime::from_unix_timestamp(*at).ok())
            .map(|at| {
                let offset = mercury_cron::job::parse_utc_offset(form.value(Field::Offset))
                    .unwrap_or(time::UtcOffset::UTC);
                format!("Próxima: {}", at.to_offset(offset))
            })
            .unwrap_or_else(|| "Agenda sem próxima ocorrência".into())
    } else {
        "Prévia: consultando agenda e fuso do servidor…".into()
    };
    put_text(
        buffer,
        area.x + 1,
        area.y + 2,
        area.width - 2,
        &next_text,
        Style::default().fg(p.subtext0),
    );
    let mut rows: Vec<(String, Option<usize>, bool)> = Vec::new();
    if form.reviewing {
        if let Ok(d) = form.definition() {
            rows.push((
                format!(
                    "Nome: {} · {}",
                    d.name,
                    if form.enabled { "ativo" } else { "pausado" }
                ),
                None,
                false,
            ));
            rows.push((
                format!(
                    "Agenda: {} · UTC {}",
                    d.schedule,
                    d.utc_offset.unwrap_or_default()
                ),
                None,
                false,
            ));
            rows.push((format!("Destino: {}", TARGETS[form.kind]), None, false));
            rows.push((
                format!(
                    "Diretório: {}",
                    d.workdir
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "padrão do servidor".into())
                ),
                None,
                false,
            ));
            rows.push((
                format!(
                    "Timeout: {}",
                    d.timeout_seconds
                        .map(|n| format!("{n}s"))
                        .unwrap_or_else(|| "padrão".into())
                ),
                None,
                false,
            ));
            let text = if form.kind == 3 {
                format!(
                    "Programa: {} · argumentos {}",
                    form.value(Field::Address),
                    form.value(Field::Args)
                )
            } else {
                format!("Instrução: {}", form.value(Field::Prompt))
            };
            for line in wrap_text(&text, (area.width - 4) as usize) {
                rows.push((line, None, false));
            }
        }
    }
    for (i, field) in form.fields().into_iter().enumerate() {
        rows.push((form.label(field).into(), Some(i), true));
        if field == Field::Days {
            for (n, day) in [
                "Domingo", "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado",
            ]
            .iter()
            .enumerate()
            {
                rows.push((
                    format!(
                        "  {} {} {day}",
                        if n == form.day_cursor { ">" } else { " " },
                        if form.days[n] { "[x]" } else { "[ ]" }
                    ),
                    Some(100 + n),
                    false,
                ));
            }
        } else if !matches!(field, Field::Review | Field::Cancel) {
            for line in wrap_text(&form.display(field), (area.width - 5) as usize) {
                rows.push((format!("  {line}"), Some(i), false));
            }
        }
        if field == Field::Preset && form.focused() == Field::Preset {
            for (n, label) in PRESETS.iter().enumerate() {
                rows.push((
                    format!("  {} {label}", if n == form.preset { "●" } else { "○" }),
                    Some(200 + n),
                    false,
                ));
            }
        }
        if field == Field::Kind && form.focused() == Field::Kind {
            for (n, label) in TARGETS.iter().enumerate() {
                rows.push((
                    format!("  {} {label}", if n == form.kind { "●" } else { "○" }),
                    Some(300 + n),
                    false,
                ));
            }
        }
        if field == Field::Address && form.kind == 2 && form.focused() == Field::Address {
            for (n, (id, label)) in form.panes.iter().enumerate() {
                rows.push((
                    format!(
                        "  {} {label} · {id}",
                        if id == form.value(Field::Address) {
                            "●"
                        } else {
                            "○"
                        }
                    ),
                    Some(500 + n),
                    false,
                ));
            }
        }
        if field == Field::Kind {
            rows.push((
                match form.kind {
                    0 | 1 => "  Nova execução em segundo plano pela sua conta conectada.",
                    2 => "  Envia a instrução ao painel escolhido; não abre novo agente.",
                    _ => "  Executa o arquivo com argumentos separados; não usa shell.",
                }
                .into(),
                None,
                false,
            ));
        }
    }
    if let Some(error) = &form.error {
        rows.push((format!("Erro: {error}"), None, false));
    }
    rows.push((
        "Próximas execuções · horário e offset do servidor:".into(),
        None,
        true,
    ));
    if form.preview_key.as_ref() == form.key().ok().as_ref() {
        for at in &form.preview {
            let offset = mercury_cron::job::parse_utc_offset(form.value(Field::Offset))
                .unwrap_or(time::UtcOffset::UTC);
            let text = time::OffsetDateTime::from_unix_timestamp(*at)
                .ok()
                .map(|t| t.to_offset(offset).to_string())
                .unwrap_or_default();
            rows.push((format!("  {text}"), None, false));
        }
    } else {
        rows.push((
            "  Pressione Tab/Enter para consultar a prévia atualizada".into(),
            None,
            false,
        ));
    }
    if form.preset == 3 {
        rows.push((
            "  Meses sem o dia escolhido são pulados.".into(),
            None,
            false,
        ));
    }
    // Footer is always visible; errors never depend on scrolling to the end.
    let footer = if form.submitting {
        "Salvando… não feche este formulário"
    } else {
        form.error.as_deref().unwrap_or(
            "Ctrl+S revisar/salvar · Esc voltar/cancelar · Alt+Enter nova linha na instrução",
        )
    };
    put_text(
        buffer,
        area.x + 1,
        area.y + area.height - 2,
        area.width - 2,
        footer,
        Style::default().fg(if form.error.is_some() {
            p.red
        } else {
            p.subtext0
        }),
    );
    let primary = if form.reviewing {
        "[Ctrl+S Salvar]"
    } else {
        "[Ctrl+S Revisar]"
    };
    let secondary = if form.reviewing {
        "[Esc Voltar]"
    } else {
        "[Esc Cancelar]"
    };
    put_text(
        buffer,
        area.x + 1,
        area.y + area.height - 1,
        area.width - 2,
        &format!("{primary}  {secondary}"),
        Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
    );
    hits.crons_rows.push((
        Rect::new(
            area.x + 1,
            area.y + area.height - 1,
            primary.len() as u16,
            1,
        ),
        1000,
    ));
    hits.crons_rows.push((
        Rect::new(
            area.x + primary.len() as u16 + 3,
            area.y + area.height - 1,
            secondary.len() as u16,
            1,
        ),
        1001,
    ));
    let height = area.height.saturating_sub(5).max(1) as usize;
    let choice = match form.focused() {
        Field::Days => Some(100 + form.day_cursor),
        Field::Preset => Some(200 + form.preset),
        Field::Kind => Some(300 + form.kind),
        _ => None,
    };
    let selected = rows
        .iter()
        .position(|(_, field, _)| choice.is_some() && *field == choice)
        .or_else(|| {
            rows.iter()
                .position(|(text, f, _)| *f == Some(form.focus) && text.contains('▏'))
                .or_else(|| rows.iter().position(|(_, f, _)| *f == Some(form.focus)))
        })
        .unwrap_or(0);
    if !form.reviewing {
        if selected < form.scroll {
            form.scroll = selected;
        } else if selected >= form.scroll + height {
            form.scroll = selected.saturating_sub(height - 1);
        }
    }
    form.scroll = form.scroll.min(rows.len().saturating_sub(height));
    for (n, (text, field, label)) in rows.iter().enumerate().skip(form.scroll).take(height) {
        let y = area.y + 3 + (n - form.scroll) as u16;
        let focused = *field == Some(form.focus) || (choice.is_some() && *field == choice);
        let style = Style::default()
            .fg(if focused {
                p.accent
            } else if *label {
                p.subtext0
            } else {
                p.text
            })
            .bg(if focused { p.active_row_bg } else { p.panel_bg });
        put_text(
            buffer,
            area.x + 1,
            y,
            area.width - 2,
            text,
            if *label {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            },
        );
        if let Some(field) = field {
            hits.crons_rows
                .push((Rect::new(area.x + 1, y, area.width - 2, 1), *field));
        }
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let mut lines = Vec::new();
    let mut row = String::new();
    let mut w = 0;
    for c in text.chars().filter(|c| !c.is_control() || *c == '\n') {
        let cw = c.width().unwrap_or(0);
        if c == '\n' || w + cw > width.max(1) {
            lines.push(std::mem::take(&mut row));
            w = 0;
        }
        if c != '\n' {
            row.push(c);
            w += cw;
        }
    }
    lines.push(row);
    lines
}

impl ClientShellState {
    pub(super) fn open_cron_form(
        &mut self,
        edit: Option<(String, CronJobDefinition)>,
        outcome: &mut ClientShellInput,
    ) {
        let snapshot = self.snapshot.as_deref();
        let cwd = snapshot
            .and_then(|s| {
                s.workspaces
                    .iter()
                    .find(|w| Some(w.workspace_id.as_str()) == s.focused_workspace_id.as_deref())
            })
            .map(|w| w.new_workspace_cwd.clone())
            .unwrap_or_default();
        let panes = snapshot
            .map(|s| {
                s.panes
                    .iter()
                    .map(|p| {
                        (
                            p.pane_id.clone(),
                            p.label.clone().unwrap_or_else(|| {
                                p.cwd.clone().unwrap_or_else(|| "Painel".into())
                            }),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.crons_view.form = Some(CronForm::new(cwd, panes, edit));
        self.crons_view.error = None;
        self.request_cron_preview(outcome);
        outcome.repaint = true;
    }
    pub(super) fn request_cron_preview(&mut self, outcome: &mut ClientShellInput) {
        let Some(f) = self.crons_view.form.as_mut() else {
            return;
        };
        let Ok(schedule) = f.schedule() else { return };
        let Ok(key) = f.key() else { return };
        if f.preview_key.as_ref() == Some(&key) || f.preview_requested.as_ref() == Some(&key) {
            return;
        }
        let offset = f.value(Field::Offset).to_string();
        f.preview_requested = Some(key.clone());
        self.push_endpoint_method_with_kind(
            Method::CronPreview(CronPreviewParams {
                schedule,
                utc_offset: (!offset.is_empty()).then_some(offset),
            }),
            PendingEndpointKind::CronPreview { key },
            outcome,
        );
    }
    pub(super) fn cron_preview_result(
        &mut self,
        key: String,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) {
        let Some(f) = self.crons_view.form.as_mut() else {
            return;
        };
        if f.key().ok().as_ref() != Some(&key) {
            // An initial server-offset response remains useful if the user has
            // already selected another preset. Do not reuse its occurrences.
            if key.starts_with(&f.generation) && f.value(Field::Offset).is_empty() {
                if let Ok(crate::api::schema::ResponseResult::CronPreview { utc_offset, .. }) =
                    result
                {
                    f.set(Field::Offset, &utc_offset);
                    f.preview_requested = None;
                }
            }
            return;
        }
        f.preview_requested = None;
        match result {
            Ok(crate::api::schema::ResponseResult::CronPreview {
                utc_offset,
                occurrences,
            }) => {
                if f.value(Field::Offset).is_empty() {
                    f.set(Field::Offset, &utc_offset);
                }
                f.preview = occurrences;
                f.preview_key = f.key().ok();
                f.error = None;
            }
            Err(e) => {
                f.error = Some(format!("Agenda: {}", e.message));
                f.preview_key = None;
            }
            _ => {
                f.error = Some("Servidor sem suporte à prévia; atualize o Mercury".into());
            }
        }
    }
    pub(super) fn handle_cron_form_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        use crossterm::event::KeyModifiers;
        let Some(f) = self.crons_view.form.as_mut() else {
            return;
        };
        outcome.repaint = true;
        if f.submitting {
            return;
        }
        let field = f.focused();
        if f.reviewing {
            match key.code {
                KeyCode::Down | KeyCode::PageDown => {
                    f.scroll =
                        f.scroll
                            .saturating_add(if key.code == KeyCode::PageDown { 10 } else { 1 });
                    return;
                }
                KeyCode::Up | KeyCode::PageUp => {
                    f.scroll =
                        f.scroll
                            .saturating_sub(if key.code == KeyCode::PageUp { 10 } else { 1 });
                    return;
                }
                _ => {}
            }
        }
        let save = key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Esc || (key.code == KeyCode::Enter && field == Field::Cancel) {
            if f.reviewing {
                f.reviewing = false;
                f.focus = 0;
                f.scroll = 0;
            } else {
                self.crons_view.form = None;
            }
            return;
        }
        if save || (key.code == KeyCode::Enter && field == Field::Review) {
            if let Some(snapshot) = self.snapshot.as_deref() {
                f.panes = snapshot
                    .panes
                    .iter()
                    .map(|p| {
                        (
                            p.pane_id.clone(),
                            p.label.clone().unwrap_or_else(|| "Painel".into()),
                        )
                    })
                    .collect();
            }
            if let Err(e) = f.definition() {
                f.error = Some(e);
                return;
            }
            if !f.reviewing {
                f.reviewing = true;
                f.focus = 0;
                f.scroll = 0;
                self.request_cron_preview(outcome);
                return;
            }
            if f.preview_key.as_ref() != f.key().ok().as_ref() {
                f.error = Some("Aguarde a prévia válida antes de salvar".into());
                self.request_cron_preview(outcome);
                return;
            }
            let definition = f.definition().expect("validated definition");
            let method = if let Some(id) = &f.job_id {
                Method::CronEdit(crate::api::schema::CronEditParams {
                    job_id: id.clone(),
                    definition,
                })
            } else {
                Method::CronAdd(crate::api::schema::CronAddParams { definition })
            };
            f.submitting = true;
            if !self.push_endpoint_method_with_kind(
                method,
                PendingEndpointKind::CronFormSave,
                outcome,
            ) {
                if let Some(f) = self.crons_view.form.as_mut() {
                    f.submitting = false;
                    f.error = Some("Endpoint indisponível".into());
                }
            }
            return;
        }
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                f.focus = (f.focus + 1) % f.fields().len();
                self.request_cron_preview(outcome);
            }
            KeyCode::BackTab | KeyCode::Up => {
                f.focus = if f.focus == 0 {
                    f.fields().len() - 1
                } else {
                    f.focus - 1
                };
                self.request_cron_preview(outcome);
            }
            KeyCode::Left | KeyCode::Right
                if matches!(
                    field,
                    Field::Preset
                        | Field::Unit
                        | Field::Kind
                        | Field::Days
                        | Field::DirectoryMode
                        | Field::Enabled
                ) || (field == Field::Address && f.kind == 2) =>
            {
                f.choice(field, if key.code == KeyCode::Left { -1 } else { 1 });
                self.request_cron_preview(outcome);
            }
            KeyCode::Char(' ') if field == Field::Days => {
                f.days[f.day_cursor] = !f.days[f.day_cursor];
                f.preview_key = None;
                self.request_cron_preview(outcome);
            }
            KeyCode::Char(' ') | KeyCode::Enter
                if matches!(
                    field,
                    Field::Preset
                        | Field::Unit
                        | Field::Kind
                        | Field::DirectoryMode
                        | Field::Enabled
                ) || (field == Field::Address && f.kind == 2) =>
            {
                f.choice(field, 1);
                self.request_cron_preview(outcome);
            }
            KeyCode::Enter
                if matches!(field, Field::Prompt | Field::Args)
                    && key.modifiers.contains(KeyModifiers::ALT) =>
            {
                f.insert("\n")
            }
            KeyCode::Enter => {
                f.focus = (f.focus + 1) % f.fields().len();
                self.request_cron_preview(outcome);
            }
            _ => {
                if let Some((_, editor)) = f.inputs.iter_mut().find(|(id, _)| *id == field) {
                    if editor.len() < 32768 {
                        editor.handle_key(key);
                    }
                    f.preview_key = None;
                }
            }
        }
    }
    pub(super) fn click_cron_form(&mut self, index: usize, outcome: &mut ClientShellInput) {
        if index >= 1000 {
            let key = crate::input::TerminalKey::new(
                if index == 1000 {
                    KeyCode::Char('s')
                } else {
                    KeyCode::Esc
                },
                if index == 1000 {
                    crossterm::event::KeyModifiers::CONTROL
                } else {
                    crossterm::event::KeyModifiers::empty()
                },
            );
            self.handle_cron_form_key(&key, outcome);
            return;
        }
        if let Some(f) = self.crons_view.form.as_mut() {
            if index >= 500 {
                if let Some((id, _)) = f.panes.get(index - 500) {
                    let id = id.clone();
                    f.set(Field::Address, &id);
                }
                outcome.repaint = true;
                return;
            }
            if index >= 300 {
                f.kind = (index - 300).min(3);
                f.preview_key = None;
                self.request_cron_preview(outcome);
                outcome.repaint = true;
                return;
            }
            if index >= 200 {
                f.preset = (index - 200).min(7);
                f.preview_key = None;
                self.request_cron_preview(outcome);
                outcome.repaint = true;
                return;
            }
            if (100..107).contains(&index) {
                f.day_cursor = index - 100;
                f.days[index - 100] = !f.days[index - 100];
                f.preview_key = None;
                self.request_cron_preview(outcome);
                outcome.repaint = true;
                return;
            }
            f.focus = index.min(f.fields().len() - 1);
        }
        let field = self.crons_view.form.as_ref().map(CronForm::focused);
        if matches!(
            field,
            Some(
                Field::Review | Field::Cancel | Field::Unit | Field::Enabled | Field::DirectoryMode
            )
        ) || (field == Some(Field::Address)
            && self.crons_view.form.as_ref().is_some_and(|f| f.kind == 2))
        {
            let key = crate::input::TerminalKey::new(
                KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            self.handle_cron_form_key(&key, outcome);
        }
        outcome.repaint = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_cover_weekdays_intervals_month_and_absolute_date() {
        let mut f = CronForm::new("/repo".into(), vec![], None);
        f.set(Field::Offset, "-03:00");
        f.set(Field::Date, "2027-03-10");
        for (preset, expected) in [
            (0, "0 9 * * 1-5"),
            (1, "0 9 * * *"),
            (2, "0 9 * * 1,2,3,4,5"),
            (3, "0 9 1 * *"),
            (4, "every 30m"),
            (5, "30m"),
            (6, "2027-03-10T09:00:00-03:00"),
            (7, "0 9 * * 1-5"),
        ] {
            f.preset = preset;
            assert_eq!(f.schedule().unwrap(), expected);
            let now = time::OffsetDateTime::parse(
                "2027-02-01T11:59:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap();
            assert!(mercury_cron::Schedule::parse(
                expected,
                now,
                mercury_cron::job::parse_utc_offset("-03:00").unwrap()
            )
            .unwrap()
            .next_after(now)
            .is_some());
        }
        f.preset = 0;
        let now = time::OffsetDateTime::parse(
            "2027-02-01T11:59:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let next = mercury_cron::Schedule::parse(
            &f.schedule().unwrap(),
            now,
            mercury_cron::job::parse_utc_offset("-03:00").unwrap(),
        )
        .unwrap()
        .next_after(now)
        .unwrap();
        assert_eq!(next.to_offset(time::UtcOffset::UTC).hour(), 12);
        f.preset = 3;
        f.set(Field::MonthDay, "31");
        let next = mercury_cron::Schedule::parse(&f.schedule().unwrap(), now, time::UtcOffset::UTC)
            .unwrap()
            .next_after(now)
            .unwrap();
        assert_eq!(next.month(), time::Month::March);
    }
    #[test]
    fn invalid_schedules_are_explained_without_creating_a_definition() {
        let mut f = CronForm::new("/repo".into(), vec![], None);
        f.set(Field::Time, "25:60");
        assert!(f.schedule().unwrap_err().contains("Hora"));
        f.preset = 2;
        f.set(Field::Time, "09:00");
        f.days = [false; 7];
        assert!(f.schedule().unwrap_err().contains("dia"));
        f.preset = 4;
        f.set(Field::Number, "0");
        assert!(f.schedule().is_err());
        f.preset = 3;
        f.set(Field::MonthDay, "32");
        assert!(f.schedule().is_err());
    }
    #[test]
    fn editing_preserves_unexposed_delivery_repeat_and_agent_options() {
        let d = CronJobDefinition {
            name: "Original".into(),
            schedule: "every 1h".into(),
            utc_offset: Some("-03:00".into()),
            target: JobTarget::Headless {
                provider: "codex".into(),
                prompt: "original".into(),
                prompt_files: vec!["/repo/task.md".into()],
                model: Some("configured-model".into()),
                effort: Some("high".into()),
                skills: vec!["review".into()],
            },
            workdir: Some("/repo".into()),
            timeout_seconds: Some(120),
            enabled: Some(false),
            catch_up: mercury_cron::CatchUp::RunOnce,
            repeat_times: Some(3),
            deliver: vec![mercury_cron::Delivery::Local { notify: true }],
        };
        let mut f = CronForm::new("/repo".into(), vec![], Some(("job-1".into(), d.clone())));
        f.set(Field::Raw, "every 2h");
        f.set(Field::Prompt, "new\ninstruction");
        let changed = f.definition().unwrap();
        assert_eq!(changed.deliver, d.deliver);
        assert_eq!(changed.repeat_times, Some(3));
        assert_eq!(changed.catch_up, d.catch_up);
        assert!(
            matches!(changed.target,JobTarget::Headless{model:Some(model),skills,prompt,..} if model=="configured-model"&&skills==["review"]&&prompt.contains('\n'))
        );
        assert_eq!(changed.enabled, Some(false));
    }
    #[test]
    fn render_is_usable_in_small_light_dark_and_uncolored_terminals() {
        let mut monochrome = Palette::mercury();
        for color in [
            &mut monochrome.accent,
            &mut monochrome.text,
            &mut monochrome.subtext0,
            &mut monochrome.active_row_bg,
            &mut monochrome.panel_bg,
            &mut monochrome.red,
        ] {
            *color = ratatui::style::Color::Reset;
        }
        for (name, palette) in [
            ("dark", Palette::mercury()),
            ("light", Palette::mercury_light()),
            ("monochrome", monochrome),
        ] {
            for (w, h) in [(80, 24), (120, 40)] {
                let mut config = ClientShellConfig::from_config(&Config::default());
                config.palette = palette.clone();
                let mut f = CronForm::new("/repo".into(), vec![], None);
                f.set(Field::Name, "Revisão diária");
                let mut buffer = Buffer::empty(Rect::new(0, 0, w, h));
                let mut hits = ShellHitMap::default();
                render(
                    &mut buffer,
                    Rect::new(0, 0, w, h),
                    &config,
                    &mut f,
                    &mut hits,
                );
                let rows: Vec<String> = (0..h)
                    .map(|y| (0..w).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                    .collect();
                let text = rows.join("\n");
                assert!(text.contains("novo agendamento"));
                assert!(text.contains("Revisão diária"));
                assert!(text.contains("Ctrl+S"));
                assert!(!hits.crons_rows.is_empty());
                if std::env::var_os("MERCURY_009_RENDER_DIR").is_some() {
                    let root = std::path::PathBuf::from(
                        std::env::var_os("MERCURY_009_RENDER_DIR").unwrap(),
                    );
                    std::fs::create_dir_all(&root).unwrap();
                    std::fs::write(root.join(format!("cron-{name}-{w}x{h}.txt")), text).unwrap();
                }
            }
        }
    }
}
