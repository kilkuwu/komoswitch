use komorebi_client::{Ring, Workspace};
use winsafe::{co::WM, msg::WndMsg};

// use crate::komo::Workspace;

pub struct UpdateWorkspaces;

impl UpdateWorkspaces {
    pub const ID: WM = unsafe { WM::from_raw(WM::APP.raw() + 1) };

    pub fn to_wmdmsg(workspaces: Ring<Workspace>) -> WndMsg {
        let data = Box::new(workspaces);
        let ptr = Box::into_raw(data) as isize;

        WndMsg {
            msg_id: Self::ID,
            wparam: 0,
            lparam: ptr,
        }
    }

    pub fn from_wndmsg(p: WndMsg) -> Ring<Workspace> {
        let workspaces = unsafe { Box::from_raw(p.lparam as *mut Ring<Workspace>) };
        *workspaces
    }
}
