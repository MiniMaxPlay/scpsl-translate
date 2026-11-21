use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub fn find_pid(exe_name: &str) -> Option<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let target = exe_name.to_ascii_lowercase();

    let mut found = None;
    unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name_len = entry
                    .szExeFile
                    .iter()
                    .position(|&value| value == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
                if name.eq_ignore_ascii_case(&target) {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    found
}

pub struct ProcessWatch {
    handle: HANDLE,
}

impl ProcessWatch {
    pub fn open(pid: u32) -> windows::core::Result<Self> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }?;
        Ok(Self { handle })
    }

    pub fn is_alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle, 0) != WAIT_OBJECT_0 }
    }
}

impl Drop for ProcessWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::find_pid;

    #[test]
    fn missing_process_is_not_found() {
        assert_eq!(find_pid("no_such_process_xyz.exe"), None);
    }

    #[test]
    fn current_process_is_found() {
        let path = std::env::current_exe().expect("current executable");
        let name = path.file_name().expect("executable name").to_string_lossy();
        assert_eq!(find_pid(&name), Some(std::process::id()));
    }
}
