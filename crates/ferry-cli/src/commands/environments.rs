//! `ferry environments …`: groups of services and datastores, each on a
//! private network of its own (production, staging…).

use anyhow::Result;
use chrono::Utc;
use ferry_core::dto::{CreateEnvironment, EnvironmentView};

use super::{Ctx, confirm, print_json};
use crate::output::{self, Cell, Table, outln};

fn names_or_dash(names: &[String]) -> String {
    if names.is_empty() { "-".to_string() } else { names.join(", ") }
}

/// The DNS records the services of an environment need, one block per
/// domain of the server that is not local.
fn print_dns(v: &EnvironmentView) -> Result<()> {
    for dns in &v.dns {
        outln!("Create this DNS record where the DNS of {} is managed, for {}:", dns.domain, dns.wildcard)?;
        let mut t = Table::new(&["TYPE", "NAME", "VALUE"]);
        for r in &dns.records {
            let value = r.value.clone().unwrap_or_else(|| "(this server's public address)".to_string());
            t.row(vec![Cell::new(r.record_type.as_str()), Cell::new(r.name.as_str()), Cell::new(value)]);
        }
        outln!("{}", t.render(output::stdout_color()).trim_end())?;
    }
    Ok(())
}

pub async fn list(ctx: &Ctx) -> Result<()> {
    let resp = ctx.client.get::<Vec<EnvironmentView>>(&["environments"], &[]).await?;
    if ctx.json {
        return print_json(&resp.raw);
    }
    let mut t = Table::new(&["NAME", "SERVICES", "DATASTORES", "URLS", "CREATED"]);
    for v in &resp.data {
        t.row(vec![
            Cell::new(v.environment.name.as_str()),
            Cell::new(v.services.len().to_string()),
            Cell::new(v.datastores.len().to_string()),
            Cell::new(v.url_pattern.as_str()),
            Cell::new(output::relative_time(v.environment.created_at, Utc::now())),
        ]);
    }
    outln!("{}", t.render(output::stdout_color()).trim_end())?;
    if resp.data.len() == 1 {
        outln!()?;
        outln!("Create another one with: ferry environments create staging")?;
    }
    Ok(())
}

pub async fn create(ctx: &Ctx, name: &str) -> Result<()> {
    let body = CreateEnvironment { name: name.to_string() };
    let resp = ctx.client.post::<_, EnvironmentView>(&["environments"], &[], &body).await?;
    if ctx.json {
        return print_json(&resp.raw);
    }
    let v = &resp.data;
    let e = &v.environment;
    outln!("Created environment '{}' ({})", e.name, e.id)?;
    outln!("Its web services are served at {}", v.url_pattern)?;
    if !v.dns.is_empty() {
        outln!()?;
        print_dns(v)?;
    }
    outln!()?;
    outln!("Create services and datastores in it with --environment {}, for example:", e.name)?;
    outln!("  ferry create web --image nginx:alpine --environment {}", e.name)?;
    outln!("  ferry db create db --environment {}", e.name)?;
    Ok(())
}

pub async fn show(ctx: &Ctx, name: &str) -> Result<()> {
    let resp = ctx.client.get::<EnvironmentView>(&["environments", name], &[]).await?;
    if ctx.json {
        return print_json(&resp.raw);
    }
    let v = &resp.data;
    let e = &v.environment;
    let pairs: Vec<(&str, Cell)> = vec![
        ("Name", Cell::new(e.name.as_str())),
        ("ID", Cell::new(e.id.as_str())),
        ("URLs", Cell::new(v.url_pattern.as_str())),
        ("Services", Cell::new(names_or_dash(&v.services))),
        ("Datastores", Cell::new(names_or_dash(&v.datastores))),
        ("Created", Cell::new(output::relative_time(e.created_at, Utc::now()))),
    ];
    outln!("{}", output::render_kv(&pairs, output::stdout_color()).trim_end())?;
    if !v.dns.is_empty() {
        outln!()?;
        print_dns(v)?;
    }
    Ok(())
}

pub async fn remove(ctx: &Ctx, name: &str, yes: bool) -> Result<()> {
    // Resolve an id to the environment's name for the prompt and messages.
    let e = ctx.client.get::<EnvironmentView>(&["environments", name], &[]).await?.data.environment;
    confirm(&format!("delete environment '{}' and its private network", e.name), yes).await?;
    ctx.client.delete_no_content(&["environments", &e.id], &[]).await?;
    if ctx.json {
        return print_json(&serde_json::json!({ "deleted": e.name, "id": e.id }));
    }
    outln!("Deleted environment '{}'.", e.name)?;
    Ok(())
}
