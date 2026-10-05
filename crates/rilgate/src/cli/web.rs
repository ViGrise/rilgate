use super::WebCommand;
use rilgate_web::config::WebConfig;

pub fn run(config: &WebConfig, command: WebCommand) -> anyhow::Result<()> {
  match command {
    WebCommand::Init { directory } => rilgate_web::management::init(&directory),
    WebCommand::Validate { directory } => rilgate_web::management::validate(config, &directory),
    WebCommand::Diff { directory } => rilgate_web::management::diff(&directory),
  }
}
