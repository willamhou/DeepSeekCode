use std::env;

#[derive(Debug)]
pub struct Cli {
    pub command: Option<Command>,
}

impl Cli {
    pub fn parse() -> Self {
        let mut args = env::args().skip(1).collect::<Vec<_>>();
        if args.is_empty() {
            return Self {
                command: Some(Command::Chat(ChatArgs::default())),
            };
        }

        let first = args.remove(0);
        let command = match first.as_str() {
            "run" => {
                let task = args.first().cloned().unwrap_or_else(|| "Run task".to_string());
                Command::Run(RunArgs { task, skill: None })
            }
            "diff" => Command::Diff(DiffArgs {}),
            "resume" => Command::Resume(ResumeArgs { session: None }),
            "config" => Command::Config(ConfigArgs {
                print_default: args.iter().any(|arg| arg == "--print-default"),
            }),
            "doctor" => Command::Doctor(DoctorArgs {}),
            _ => {
                let task = std::iter::once(first).chain(args).collect::<Vec<_>>().join(" ");
                let task = if task.is_empty() { None } else { Some(task) };
                Command::Chat(ChatArgs { task, skill: None })
            }
        };

        Self {
            command: Some(command),
        }
    }
}

#[derive(Debug)]
pub enum Command {
    Chat(ChatArgs),
    Run(RunArgs),
    Diff(DiffArgs),
    Resume(ResumeArgs),
    Config(ConfigArgs),
    Doctor(DoctorArgs),
}

impl Default for Command {
    fn default() -> Self {
        Self::Chat(ChatArgs::default())
    }
}

#[derive(Debug, Default)]
pub struct ChatArgs {
    pub task: Option<String>,
    pub skill: Option<String>,
}

#[derive(Debug)]
pub struct RunArgs {
    pub task: String,
    pub skill: Option<String>,
}

#[derive(Debug)]
pub struct DiffArgs {}

#[derive(Debug)]
pub struct ResumeArgs {
    pub session: Option<String>,
}

#[derive(Debug)]
pub struct ConfigArgs {
    pub print_default: bool,
}

#[derive(Debug)]
pub struct DoctorArgs {}
