// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if keepitlocal_lib::maybe_run_archive_worker_from_args()
        || keepitlocal_lib::maybe_run_index_worker_from_args()
    {
        return;
    }

    keepitlocal_lib::run()
}
