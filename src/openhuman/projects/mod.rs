pub mod bus;
mod ops;
pub(crate) mod run_registry;
pub mod scheduler;
mod schemas;
pub mod session_watcher;
pub(crate) mod store;
pub mod tools;
mod types;

pub use bus::register_project_ai_runner;
pub use scheduler::{start_throttle_poller, try_dispatch};
pub use session_watcher::register_session_watch;

pub use ops::{
    add_attachment, add_comment, create_subtask, create_task, delete_attachment, delete_subtask,
    delete_task, get_board, list_attachments, list_subtasks, list_task_events, move_task,
    update_bucket, update_task, BucketWithTasks, BucketsWithTasks, CreateTaskInput,
};
pub use schemas::{
    all_controller_schemas as all_projects_controller_schemas,
    all_registered_controllers as all_projects_registered_controllers,
};
pub(crate) use store::{
    cleanup_stale_running_task_runs, ensure_default_project, finish_task_run, get_project,
    get_task, insert_running_run, list_buckets, list_runs_for_task, list_task_runs, list_tasks,
};
pub use tools::{
    ProjectsAddAttachmentTool, ProjectsCompleteTaskTool, ProjectsCreateTaskTool,
    ProjectsListTaskRunsTool, ProjectsListTool, ProjectsMoveTaskTool, ProjectsReadAttachmentTool,
};
pub use types::{
    Bucket, BucketPatch, FeishuSessionBinding, Project, ProjectTaskRun, Task, TaskAttachment,
    TaskEvent, TaskEventKind, TaskPatch,
};
