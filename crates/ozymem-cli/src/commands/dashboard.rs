use crate::client::build_backend_client;
use ozymem_core::LessonRecord;
use ozymem_parser::FileDefinitionMap;
use crate::client::BackendClient;

pub fn centered_rect(percent_x: u16, percent_y: u16, r: ratatui::prelude::Rect) -> ratatui::prelude::Rect {
    let popup_layout = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length((r.height * (100 - percent_y)) / 200),
        ratatui::layout::Constraint::Length((r.height * percent_y) / 100),
        ratatui::layout::Constraint::Length((r.height * (100 - percent_y)) / 200),
    ])
    .split(r);

    ratatui::layout::Layout::horizontal([
        ratatui::layout::Constraint::Length((r.width * (100 - percent_x)) / 200),
        ratatui::layout::Constraint::Length((r.width * percent_x) / 100),
        ratatui::layout::Constraint::Length((r.width * (100 - percent_x)) / 200),
    ])
    .split(popup_layout[1])[1]
}


pub async fn run_dashboard() -> anyhow::Result<()> {
    use ratatui::{
        backend::CrosstermBackend,
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
        Terminal,
    };

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum ActiveTab {
        Memories,
        SystemStatus,
        GraphPRs,
    }

    // 1. Setup terminal raw mode
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    
    struct CleanupTerminal;
    impl Drop for CleanupTerminal {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            );
        }
    }
    let _cleanup = CleanupTerminal;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // 2. Setup backend connection
    let connection = build_backend_client().await?;
    let display_uri = connection.display_uri();
    
    // 3. Application State variables
    let mut active_tab = ActiveTab::Memories;
    
    // Tab 1: Memories state
    let project_path = if let Ok(cwd) = std::env::current_dir() {
        cwd.to_string_lossy().to_string()
    } else {
        ".".to_string()
    };
    let db_path = std::path::Path::new(&project_path)
        .join(".ozymem")
        .join("vectors")
        .join("vectors.json");
        
    let mut records: Vec<crate::mcp::VectorRecord> = if db_path.exists() {
        let data = std::fs::read_to_string(&db_path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut last_db_mtime = db_path.metadata().ok().and_then(|m| m.modified().ok());
    
    let mut selected_index = 0;
    let mut scroll_offset = 0;
    let mut search_query = String::new();
    let mut input_mode = false;
    let mut prune_confirm = false;
    let mut prune_list: Vec<String> = Vec::new();
    
    // Tab 2: System Status & Watchers state
    let mut status_ping_ok = Some(connection.ping().await.is_ok());
    let mut sorted_projects: Vec<(String, String)> = Vec::new();
    let mut selected_project_idx = 0;
    let mut log_lines: Vec<String> = Vec::new();
    
    // Tab 3: Graph PRs state
    let _gpr_list: Vec<()> = Vec::new();
    let _selected_gpr_idx = 0;
    let _active_gpr_details: Option<(String, String, Vec<FileDefinitionMap>, Vec<LessonRecord>)> = None;
    let _gpr_scroll_offset = 0;
    
    let mut status_message = "Bienvenido a OzyMem Dashboard! Pulse 1, 2 o 3 para navegar por pestañas.".to_string();
    
    // Initial fetches
    if let Ok(reg) = ozymem_core::registry::ProjectRegistry::open() {
        if let Ok(projects) = reg.list_projects() {
            let mut projs: Vec<(String, String)> = projects.into_iter().map(|p| (p.name, p.path)).collect();
            projs.sort_by(|a, b| a.0.cmp(&b.0));
            sorted_projects = projs;
        }
    }
    
    // Helper function to load selected project logs
    let load_current_project_logs = |sorted_projects: &[(String, String)], idx: usize| -> Vec<String> {
        if let Some((name, _)) = sorted_projects.get(idx) {
            let home_dir = home::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let log_file = home_dir.join(format!(".ozymem-{}.log", name));
            if log_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&log_file) {
                    let mut lines = content.lines()
                        .rev()
                        .take(50)
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>();
                    lines.reverse();
                    return lines;
                }
            }
        }
        vec!["No se encontraron bitacoras para este proyecto.".to_string()]
    };
    
    if !sorted_projects.is_empty() {
        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
    }
    
    // Helper function to fetch GPR details (no-op in SQLite mode)
    let _fetch_gpr_details_sync = |_connection: &BackendClient, _gpr_id: i64| -> Option<(String, String, Vec<FileDefinitionMap>, Vec<LessonRecord>)> {
        None
    };
    
    loop {
        // Auto-reload de vectors.json si hay cambios en disco
        if let Ok(metadata) = std::fs::metadata(&db_path) {
            if let Ok(mtime) = metadata.modified() {
                if Some(mtime) != last_db_mtime {
                    last_db_mtime = Some(mtime);
                    if let Ok(data) = std::fs::read_to_string(&db_path) {
                        if let Ok(parsed) = serde_json::from_str::<Vec<crate::mcp::VectorRecord>>(&data) {
                            records = parsed;
                            status_message = "Recuerdos vectoriales actualizados automáticamente.".to_string();
                        }
                    }
                }
            }
        }

        // Filter records by search query and schema_version == 1
        let filtered_records: Vec<&crate::mcp::VectorRecord> = records.iter()
            .filter(|r| r.schema_version == 1)
            .filter(|r| {
                if search_query.is_empty() {
                    true
                } else {
                    r.text.to_lowercase().contains(&search_query.to_lowercase())
                        || r.source_path.to_lowercase().contains(&search_query.to_lowercase())
                        || r.category.to_lowercase().contains(&search_query.to_lowercase())
                        || r.id.to_lowercase().contains(&search_query.to_lowercase())
                }
            })
            .collect();
            
        if selected_index >= filtered_records.len() && !filtered_records.is_empty() {
            selected_index = filtered_records.len() - 1;
        }
        
        terminal.draw(|f| {
            let size = f.size();
            
            // Layout: Title Block (3 lines), Main Area (flexible), Bottom Area (4 lines)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(4),
                ])
                .split(size);
                
            // 1. Title Block with Tabs (No Emojis)
            let tabs_items = vec![
                Line::from(" [1] Recuerdos "),
                Line::from(" [2] Monitoreo y Watchers "),
                Line::from(" (Graph PRs no disponible) "),
            ];
            
            let selected_tab_idx = match active_tab {
                ActiveTab::Memories => 0,
                ActiveTab::SystemStatus => 1,
                ActiveTab::GraphPRs => 2,
            };
            
            let header_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(45),
                    Constraint::Percentage(55),
                ])
                .split(chunks[0]);
                
            let title_para = Paragraph::new(Line::from(vec![
                Span::styled(" OZYMEM ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("INTELLIGENT HYBRID VECTOR STORE ", Style::default().fg(Color::White)),
                Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray)),
            ]))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(title_para, header_layout[0]);
            
            let tabs_widget = ratatui::widgets::Tabs::new(tabs_items)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                .select(selected_tab_idx)
                .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            f.render_widget(tabs_widget, header_layout[1]);
            
            // 2. Main Area (Depends on Active Tab)
            match active_tab {
                ActiveTab::Memories => {
                    let main_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Percentage(60),
                        ])
                        .split(chunks[1]);
                        
                    // 2a. Left: List of memories
                    let list_title = if search_query.is_empty() {
                        format!(" Recuerdos ({}) ", filtered_records.len())
                    } else {
                        format!(" Busqueda: '{}' ({}) ", search_query, filtered_records.len())
                    };
                    
                    let list_block = Block::default()
                        .title(list_title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(if input_mode { Color::Yellow } else { Color::White }));
                        
                    let items: Vec<ListItem> = filtered_records.iter().enumerate().map(|(idx, r)| {
                        let symbol_name = r.id.split("::").last().unwrap_or(&r.id);
                        let base_name = std::path::Path::new(&r.source_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&r.source_path);
                            
                        let bg_color = if idx == selected_index { Color::Rgb(30, 60, 90) } else { Color::Reset };
                        let is_selected_style = if idx == selected_index {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        
                        let cat_color = match r.category.to_lowercase().as_str() {
                            "lesson" => Color::LightRed,
                            "fact" => Color::LightGreen,
                            _ => Color::LightBlue,
                        };
                        
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {:02} ", idx + 1), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{:<15} ", base_name), Style::default().fg(Color::Cyan)),
                            Span::styled(format!(" [{}] ", r.category.to_uppercase()), Style::default().fg(cat_color)),
                            Span::styled(symbol_name.to_string(), is_selected_style),
                        ]))
                        .style(Style::default().bg(bg_color))
                    }).collect();
                    
                    if items.is_empty() {
                        let empty_para = Paragraph::new("No se encontraron recuerdos en la base de datos.")
                            .alignment(ratatui::layout::Alignment::Center)
                            .block(list_block);
                        f.render_widget(empty_para, main_chunks[0]);
                    } else {
                        let mut list_state = ListState::default();
                        list_state.select(Some(selected_index));
                        let list_widget = List::new(items)
                            .block(list_block)
                            .highlight_symbol(">> ")
                            .highlight_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 60, 90)));
                        f.render_stateful_widget(list_widget, main_chunks[0], &mut list_state);
                    }
                    
                    // 2b. Right: Details pane
                    let details_block = Block::default()
                        .title(" Detalles del Recuerdo ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                        
                    if let Some(r) = filtered_records.get(selected_index) {
                        let mut details_text = Vec::new();
                        details_text.push(Line::from(vec![
                            Span::styled("ID:          ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.id.clone(), Style::default().fg(Color::Yellow)),
                        ]));
                        details_text.push(Line::from(vec![
                            Span::styled("Procedencia: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.source_path.clone(), Style::default().fg(Color::Cyan)),
                        ]));
                        
                        let cat_color = match r.category.to_lowercase().as_str() {
                            "lesson" => Color::LightRed,
                            "fact" => Color::LightGreen,
                            _ => Color::LightBlue,
                        };
                        
                        details_text.push(Line::from(vec![
                            Span::styled("Categoria:   ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.category.to_uppercase(), Style::default().fg(cat_color).add_modifier(Modifier::BOLD)),
                            Span::styled("  |  Hits: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.hit_count.to_string(), Style::default().fg(Color::LightYellow)),
                            Span::styled("  |  Esquema: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.schema_version.to_string(), Style::default().fg(Color::White)),
                        ]));
                        details_text.push(Line::from(vec![
                            Span::styled("Fecha (Unix):", Style::default().fg(Color::DarkGray)),
                            Span::styled(r.timestamp.to_string(), Style::default().fg(Color::White)),
                        ]));
                        if let Some(p_id) = &r.parent_id {
                            details_text.push(Line::from(vec![
                                Span::styled("Padre ID:    ", Style::default().fg(Color::DarkGray)),
                                Span::styled(p_id.clone(), Style::default().fg(Color::Magenta)),
                            ]));
                        }
                        details_text.push(Line::from(""));
                        details_text.push(Line::from(Span::styled("Contenido / Codigo:", Style::default().add_modifier(Modifier::UNDERLINED))));
                        details_text.push(Line::from(""));
                        
                        for line_str in r.text.lines() {
                            details_text.push(Line::from(line_str));
                        }
                        
                        let para = Paragraph::new(details_text)
                            .block(details_block)
                            .scroll((scroll_offset, 0))
                            .wrap(Wrap { trim: false });
                            
                        f.render_widget(para, main_chunks[1]);
                    } else {
                        let para = Paragraph::new("Selecciona un recuerdo para ver sus detalles.")
                            .alignment(ratatui::layout::Alignment::Center)
                            .block(details_block);
                        f.render_widget(para, main_chunks[1]);
                    }
                }
                ActiveTab::SystemStatus => {
                    let main_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Percentage(60),
                        ])
                        .split(chunks[1]);
                        
                    // 2a. Left: List of projects
                    let proj_block = Block::default()
                        .title(" Proyectos Configurados ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                        
                    let items: Vec<ListItem> = sorted_projects.iter().enumerate().map(|(idx, (name, path))| {
                        let bg_color = if idx == selected_project_idx { Color::Rgb(30, 60, 90) } else { Color::Reset };
                        let is_selected_style = if idx == selected_project_idx {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {:02} ", idx + 1), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{:<15} ", name), is_selected_style),
                            Span::styled(path.clone(), Style::default().fg(Color::DarkGray)),
                        ]))
                        .style(Style::default().bg(bg_color))
                    }).collect();
                    
                    if items.is_empty() {
                        let empty_para = Paragraph::new("No hay proyectos registrados en ozymem.toml")
                            .alignment(ratatui::layout::Alignment::Center)
                            .block(proj_block);
                        f.render_widget(empty_para, main_chunks[0]);
                    } else {
                        let mut list_state = ListState::default();
                        list_state.select(Some(selected_project_idx));
                        let list_widget = List::new(items)
                            .block(proj_block)
                            .highlight_symbol(">> ")
                            .highlight_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 60, 90)));
                        f.render_stateful_widget(list_widget, main_chunks[0], &mut list_state);
                    }
                    
                    // 2b. Right: Connection details and log tail
                    let logs_block = Block::default()
                        .title(" Monitoreo y Logs en Vivo ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                        
                    let mut status_lines = Vec::new();
                    status_lines.push(Line::from(vec![
                        Span::styled("Backend DB URL:  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(display_uri.clone(), Style::default().fg(Color::Cyan)),
                    ]));
                    
                    let ping_style = match status_ping_ok {
                        Some(true) => Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                        Some(false) => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                        None => Style::default().fg(Color::DarkGray),
                    };
                    let ping_text = match status_ping_ok {
                        Some(true) => "ACTIVO / RESPONDIENDO PING",
                        Some(false) => "INACTIVO / SIN CONEXION",
                        None => "CARGANDO...",
                    };
                    status_lines.push(Line::from(vec![
                        Span::styled("Estado Conexion:  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(ping_text, ping_style),
                    ]));
                    status_lines.push(Line::from(""));
                    status_lines.push(Line::from(Span::styled("Ultimas 50 lineas de Bitacora:", Style::default().add_modifier(Modifier::UNDERLINED))));
                    status_lines.push(Line::from(""));
                    
                    for line_str in &log_lines {
                        status_lines.push(Line::from(line_str.as_str()));
                    }
                    
                    let para = Paragraph::new(status_lines)
                        .block(logs_block)
                        .wrap(Wrap { trim: false });
                    f.render_widget(para, main_chunks[1]);
                }
                ActiveTab::GraphPRs => {
                    let gpr_block = Block::default()
                        .title(" Graph PRs ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));

                    let para = Paragraph::new(
                        "GPR functionality is not available in local mode."
                    )
                    .alignment(ratatui::layout::Alignment::Center)
                    .block(gpr_block);
                    f.render_widget(para, chunks[1]);
                }
            }
            
            // 3. Stats / Controls Block
            let stats_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(60),
                    Constraint::Percentage(40),
                ])
                .split(chunks[2]);
                
            let stats_block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray));
            
            let input_info = if input_mode {
                format!("BUSCAR (Escribe y pulsa Enter): {}", search_query)
            } else {
                match active_tab {
                    ActiveTab::Memories => "[q] Salir  [Tab] Ciclador  [s] Buscar  [f] Olvidar  [p] Depurar  [Esc] Limpiar  [,] Subir  [.] Bajar".to_string(),
                    ActiveTab::SystemStatus => "[q] Salir  [Tab] Ciclador  [r] Recargar Logs  [↑/↓] Navegar proyectos".to_string(),
                    ActiveTab::GraphPRs => "[q] Salir  [Tab] Ciclador".to_string(),
                }
            };
            
            let input_style = if input_mode {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            
            let cmd_para = Paragraph::new(vec![
                Line::from(Span::styled(input_info, input_style)),
                Line::from(Span::styled(status_message.clone(), Style::default().fg(Color::Gray))),
            ])
            .block(stats_block.clone().title(" Atajos y Estatus "));
            f.render_widget(cmd_para, stats_chunks[0]);
            
            let total_hits: i64 = records.iter().map(|r| r.hit_count).sum();
            let num_lessons = records.iter().filter(|r| r.category.eq_ignore_ascii_case("lesson")).count();
            let num_facts = records.iter().filter(|r| r.category.eq_ignore_ascii_case("fact")).count();
            let num_contexts = records.iter().filter(|r| r.category.eq_ignore_ascii_case("context")).count();
            
            let stats_lines = vec![
                Line::from(vec![
                    Span::styled("Total: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(records.len().to_string(), Style::default().fg(Color::White)),
                    Span::styled("  Facts: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(num_facts.to_string(), Style::default().fg(Color::LightGreen)),
                    Span::styled("  Lessons: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(num_lessons.to_string(), Style::default().fg(Color::LightRed)),
                ]),
                Line::from(vec![
                    Span::styled("Contexts: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(num_contexts.to_string(), Style::default().fg(Color::LightBlue)),
                    Span::styled("  Hits Acumulados: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(total_hits.to_string(), Style::default().fg(Color::LightYellow)),
                ]),
            ];
            
            let stats_para = Paragraph::new(stats_lines)
                .block(stats_block.title(" Metricas de Memoria "));
            f.render_widget(stats_para, stats_chunks[1]);
            
            if prune_confirm {
                let block = Block::default()
                    .title(" Depuracion de Huerfanos ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightRed));
                    
                let text = vec![
                    Line::from(format!("Se han detectado {} recuerdos huerfanos.", prune_list.len())),
                    Line::from("¿Deseas eliminarlos de forma definitiva?"),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(" [y] Si, aplicar depuracion ", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
                        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(" [n] Cancelar ", Style::default().fg(Color::White)),
                    ]),
                ];
                
                let area = centered_rect(60, 25, size);
                f.render_widget(Clear, area);
                let popup_para = Paragraph::new(text)
                    .block(block)
                    .alignment(ratatui::layout::Alignment::Center);
                f.render_widget(popup_para, area);
            }
        })?;
        
        // 4. Event loop poll
        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }
                if prune_confirm {
                    match key.code {
                        crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                            records.retain(|r| !prune_list.contains(&r.id));
                            if let Ok(serialized) = serde_json::to_string_pretty(&records) {
                                let _ = std::fs::write(&db_path, serialized);
                                status_message = format!("Depuracion ejecutada. {} recuerdos eliminados.", prune_list.len());
                            } else {
                                status_message = "Error al guardar los cambios en vectors.json".to_string();
                            }
                            prune_confirm = false;
                        }
                        _ => {
                            status_message = "Depuracion cancelada.".to_string();
                            prune_confirm = false;
                        }
                    }
                    continue;
                }
                
                if input_mode {
                    match key.code {
                        crossterm::event::KeyCode::Enter => {
                            input_mode = false;
                            selected_index = 0;
                            status_message = format!("Busqueda aplicada: '{}'", search_query);
                        }
                        crossterm::event::KeyCode::Esc => {
                            input_mode = false;
                            search_query.clear();
                            status_message = "Busqueda cancelada.".to_string();
                        }
                        crossterm::event::KeyCode::Backspace => {
                            search_query.pop();
                        }
                        crossterm::event::KeyCode::Char(c) => {
                            search_query.push(c);
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc => {
                            break;
                        }
                        crossterm::event::KeyCode::Tab => {
                            active_tab = match active_tab {
                                ActiveTab::Memories => ActiveTab::SystemStatus,
                                ActiveTab::SystemStatus => ActiveTab::Memories,
                                ActiveTab::GraphPRs => ActiveTab::Memories,
                            };
                            status_message = format!("Pestana activa: {:?}", active_tab);
                            
                            // Load corresponding data on tab switch
                            if active_tab == ActiveTab::SystemStatus {
                                status_ping_ok = Some(connection.ping().await.is_ok());
                                if !sorted_projects.is_empty() {
                                    log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                }
                            }
                        }
                        crossterm::event::KeyCode::Char('1') => {
                            active_tab = ActiveTab::Memories;
                            status_message = "Pestana activa: Recuerdos".to_string();
                        }
                        crossterm::event::KeyCode::Char('2') => {
                            active_tab = ActiveTab::SystemStatus;
                            status_message = "Pestana activa: Monitoreo y Watchers".to_string();
                            status_ping_ok = Some(connection.ping().await.is_ok());
                            if !sorted_projects.is_empty() {
                                log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                            }
                        }
                        crossterm::event::KeyCode::Char('3') => {
                            status_message = "Graph PRs no disponible en modo local.".to_string();
                        }
                        crossterm::event::KeyCode::Char('r') | crossterm::event::KeyCode::Char('R') => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if db_path.exists() {
                                        if let Ok(data) = std::fs::read_to_string(&db_path) {
                                            records = serde_json::from_str(&data).unwrap_or_default();
                                            status_message = "Recuerdos vectoriales recargados.".to_string();
                                        }
                                    }
                                }
                                ActiveTab::SystemStatus => {
                                    status_ping_ok = Some(connection.ping().await.is_ok());
                                    if !sorted_projects.is_empty() {
                                        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                    }
                                    status_message = "Monitoreo y logs actualizados.".to_string();
                                }
                                ActiveTab::GraphPRs => {
                                    status_message = "Graph PRs no disponible en modo local.".to_string();
                                }
                            }
                        }
                        crossterm::event::KeyCode::Up => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if selected_index > 0 {
                                        selected_index -= 1;
                                        scroll_offset = 0;
                                    }
                                }
                                ActiveTab::SystemStatus => {
                                    if selected_project_idx > 0 {
                                        selected_project_idx -= 1;
                                        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                    }
                                }
                                ActiveTab::GraphPRs => {}
                            }
                        }
                        crossterm::event::KeyCode::Down => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if selected_index + 1 < filtered_records.len() {
                                        selected_index += 1;
                                        scroll_offset = 0;
                                    }
                                }
                                ActiveTab::SystemStatus => {
                                    if selected_project_idx + 1 < sorted_projects.len() {
                                        selected_project_idx += 1;
                                        log_lines = load_current_project_logs(&sorted_projects, selected_project_idx);
                                    }
                                }
                                ActiveTab::GraphPRs => {}
                            }
                        }
                        crossterm::event::KeyCode::Char(',') => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    if scroll_offset > 0 {
                                        scroll_offset -= 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                        crossterm::event::KeyCode::Char('.') => {
                            match active_tab {
                                ActiveTab::Memories => {
                                    scroll_offset += 1;
                                }
                                _ => {}
                            }
                        }
                        crossterm::event::KeyCode::Char('s') | crossterm::event::KeyCode::Char('S') => {
                            if active_tab == ActiveTab::Memories {
                                input_mode = true;
                                search_query.clear();
                                status_message = "Escribe para buscar...".to_string();
                            }
                        }
                        crossterm::event::KeyCode::Char('f') | crossterm::event::KeyCode::Char('F') => {
                            if active_tab == ActiveTab::Memories {
                                if let Some(r) = filtered_records.get(selected_index) {
                                    let id_to_delete = r.id.clone();
                                    records.retain(|rec| rec.id != id_to_delete);
                                    if let Ok(serialized) = serde_json::to_string_pretty(&records) {
                                        let _ = std::fs::write(&db_path, serialized);
                                        status_message = format!("Olvidado: {}", id_to_delete);
                                    } else {
                                        status_message = "Error al guardar cambios en vectors.json".to_string();
                                    }
                                }
                            }
                        }
                        crossterm::event::KeyCode::Char('p') | crossterm::event::KeyCode::Char('P') => {
                            if active_tab == ActiveTab::Memories {
                                prune_list.clear();
                                for r in &records {
                                    if !std::path::Path::new(&r.source_path).exists() {
                                        prune_list.push(r.id.clone());
                                    }
                                }
                                if prune_list.is_empty() {
                                    status_message = "No se detectaron recuerdos huerfanos.".to_string();
                                } else {
                                    prune_confirm = true;
                                }
                            }
                        }
                        crossterm::event::KeyCode::Char('m') | crossterm::event::KeyCode::Char('M') => {
                            if active_tab == ActiveTab::GraphPRs {
                                status_message = "GPR merge no disponible en modo local.".to_string();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    Ok(())
}
