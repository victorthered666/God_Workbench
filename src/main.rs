use std::sync::Arc;
use gdd_harness::{agent::runtime::AgentRuntime, config::Config, http::routes, repository::Repository, runner::Runner, secrets::SecretStore, workflow::Workflow};

#[tokio::main]
async fn main() -> gdd_harness::Result<()> {
    let config=Config::from_env()?;
    config.init()?;
    let repo=Repository::new(config.database())?;
    let secrets=SecretStore::new(&config.data_dir)?;
    let agents=Arc::new(AgentRuntime::new(repo.clone(),secrets.clone(),&config.root));
    let workflow=Workflow::new(repo.clone(),agents.clone(),config.clone());
    let runner=Runner::new(workflow.clone());
    let app=routes::app(repo,secrets,workflow,runner,agents,config.root.clone());
    let address=std::net::SocketAddr::from(([127,0,0,1],config.port));
    let listener=tokio::net::TcpListener::bind(address).await?;
    println!("神的使者 工作台：http://{address}");
    if std::env::var_os("GDD_NO_BROWSER").is_none() {
        let url=format!("http://{address}");
        std::thread::spawn(move||{ let _=open::that(url); });
    }
    axum::serve(listener,app).await.map_err(|error|gdd_harness::AppError::Other(error.to_string()))
}
