use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum VectorSubcommand {
    Search {
        query: String,
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
        #[arg(short, long)]
        category: Option<String>,
    },
    List {
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        category: Option<String>,
    },
    Inspect {
        id: String,
    },
    Forget {
        id: String,
    },
    Prune {
        #[arg(long)]
        apply: bool,
    },
    Top {
        #[arg(short, long)]
        project: Option<String>,
    },
}



pub async fn run_vector_subcommand(subcommand: &VectorSubcommand) -> anyhow::Result<()> {
    // Determine target project path
    let project_path = if let Ok(cwd) = std::env::current_dir() {
        cwd.to_string_lossy().to_string()
    } else {
        ".".to_string()
    };
    
    let db_path = std::path::Path::new(&project_path)
        .join(".ozymem")
        .join("vectors")
        .join("vectors.json");

    match subcommand {
        VectorSubcommand::Search { query, limit, category } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe en: {:?}", db_path);
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;

            // Pre-filtering by category and strictly filtering by schema_version == 1
            if let Some(cat) = category {
                records.retain(|r| r.category.eq_ignore_ascii_case(cat));
            }
            records.retain(|r| r.schema_version == 1);

            let query_emb = crate::mcp::get_embedding(query).await?;
            
            type SearchMatch = (String, String, f32, String); // (id, source, score, category)
            let mut matches: Vec<SearchMatch> = Vec::new();
            for r in records {
                let score = crate::mcp::cosine_similarity(&query_emb, &r.embedding);
                matches.push((r.id, r.source_path, score, r.category));
            }

            matches.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            matches.truncate(*limit);

            use comfy_table::Table;
            let mut table = Table::new();
            table.set_header(vec!["ID", "Procedencia (Source)", "Categoría", "Similitud"]);
            for (id, source, score, cat) in matches {
                table.add_row(vec![id, source, cat, format!("{:.4}", score)]);
            }
            println!("{}", table);
        }
        VectorSubcommand::List { project, category } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe en: {:?}", db_path);
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;

            if let Some(proj) = project {
                records.retain(|r| r.project.eq_ignore_ascii_case(proj));
            }
            if let Some(cat) = category {
                records.retain(|r| r.category.eq_ignore_ascii_case(cat));
            }

            use comfy_table::Table;
            let mut table = Table::new();
            table.set_header(vec!["ID", "Procedencia (Source)", "Categoría", "Impactos (Hits)", "Fecha"]);
            for r in records {
                let date_str = format!("{}", r.timestamp);
                table.add_row(vec![r.id, r.source_path, r.category, r.hit_count.to_string(), date_str]);
            }
            println!("{}", table);
        }
        VectorSubcommand::Inspect { id } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;
            if let Some(r) = records.iter().find(|rec| &rec.id == id) {
                println!("=========================================");
                println!("INSPECCIÓN DE RECUERDO VECTORIAL");
                println!("=========================================");
                println!("ID:           {}", r.id);
                println!("Proyecto:     {}", r.project);
                println!("Categoría:    {}", r.category);
                println!("Procedencia:  {}", r.source_path);
                println!("Versión Esq:  {}", r.schema_version);
                println!("Padre ID:     {}", r.parent_id.as_deref().unwrap_or("None"));
                println!("Impactos:     {}", r.hit_count);
                println!("Fecha (Unix): {}", r.timestamp);
                println!("-----------------------------------------");
                println!("Texto:\n{}", r.text);
                println!("=========================================");
            } else {
                println!("No se encontró ningún recuerdo con ID: {}", id);
            }
        }
        VectorSubcommand::Forget { id } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;
            let initial_len = records.len();
            records.retain(|r| &r.id != id);
            if records.len() < initial_len {
                let serialized = serde_json::to_string_pretty(&records)?;
                std::fs::write(&db_path, serialized)?;
                println!("[SUCCESS] Recuerdo '{}' eliminado de la base de datos vectorial.", id);
            } else {
                println!("No se encontró ningún recuerdo con ID: {}", id);
            }
        }
        VectorSubcommand::Prune { apply } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;
            let initial_len = records.len();
            
            let mut orphans = Vec::new();
            for r in &records {
                if !std::path::Path::new(&r.source_path).exists() {
                    orphans.push(r.id.clone());
                }
            }

            if orphans.is_empty() {
                println!("No se encontraron recuerdos huérfanos o inactivos para depurar.");
                return Ok(());
            }

            println!("Vectores huérfanos detectados (cuyo archivo de origen ya no existe):");
            for id in &orphans {
                println!("  - {}", id);
            }

            if *apply {
                records.retain(|r| !orphans.contains(&r.id));
                let serialized = serde_json::to_string_pretty(&records)?;
                std::fs::write(&db_path, serialized)?;
                println!("[SUCCESS] Depuración ejecutada. {} recuerdos eliminados.", initial_len - records.len());
            } else {
                println!("\n[DRY RUN] Ejecuta con '--apply' para eliminar estos recuerdos.");
            }
        }
        VectorSubcommand::Top { project } => {
            if !db_path.exists() {
                println!("La base de datos vectorial no existe.");
                return Ok(());
            }
            let data = std::fs::read_to_string(&db_path)?;
            let mut records: Vec<crate::mcp::VectorRecord> = serde_json::from_str(&data)?;

            if let Some(proj) = project {
                records.retain(|r| r.project.eq_ignore_ascii_case(proj));
            }

            records.sort_by(|a, b| b.hit_count.cmp(&a.hit_count));
            records.truncate(10);

            use comfy_table::Table;
            let mut table = Table::new();
            table.set_header(vec!["ID", "Procedencia", "Categoría", "Impactos (Hits)"]);
            for r in records {
                table.add_row(vec![r.id, r.source_path, r.category, r.hit_count.to_string()]);
            }
            println!("{}", table);
        }
    }
    Ok(())
}

