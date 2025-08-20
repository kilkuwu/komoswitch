use crate::komo::Workspace;

pub struct ChangedWorkspace {
    pub data: Workspace,
    pub name_changed: bool,
    pub state_changed: bool,
}

pub struct Workspaces {
    pub data: Vec<ChangedWorkspace>,
}

impl Workspaces {
    pub fn new() -> Self {
        let mut res = Self { data: Vec::new() };
        loop {
            let Ok(new_workspaces) = crate::komo::read_workspaces() else {
                log::debug!("Failed to read workspaces, retrying...");
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            };
            res.try_update(new_workspaces);
            break;
        }
        res
    }

    pub fn try_update(&mut self, workspaces: Vec<Workspace>) -> bool {
        if self.data.len() == workspaces.len() {
            let mut changed = false;
            for (i, workspace) in workspaces.iter().enumerate() {
                let current = &mut self.data[i];
                if current.data.name != workspace.name {
                    current.data.name = workspace.name.clone();
                    current.name_changed = true;
                    changed = true;
                }
                if current.data.state != workspace.state {
                    current.data.state = workspace.state.clone();
                    current.state_changed = true;
                    changed = true;
                }
            }
            changed
        } else {
            self.data = workspaces
                .into_iter()
                .map(|ws| ChangedWorkspace {
                    data: ws,
                    name_changed: true,
                    state_changed: true,
                })
                .collect();
            true
        }
    }

    pub fn name_changed(&self) -> bool {
        self.data.iter().any(|ws| ws.name_changed)
    }
}
