mod ops;
mod schemas;
pub(crate) mod store;
mod types;
pub mod tools;

pub use ops::{
    add_attachment, add_comment, create_task, delete_attachment, delete_task, get_board,
    list_attachments, list_task_events, move_task, update_bucket, update_task,
    BucketWithTasks, BucketsWithTasks, CreateTaskInput,
};
pub use schemas::{
    all_controller_schemas as all_projects_controller_schemas,
    all_registered_controllers as all_projects_registered_controllers,
};
pub use tools::{
    ProjectsAddAttachmentTool, ProjectsCompleteTaskTool, ProjectsCreateTaskTool,
    ProjectsListTool, ProjectsMoveTaskTool, ProjectsReadAttachmentTool,
};
pub use types::{Bucket, BucketPatch, Project, Task, TaskAttachment, TaskEvent, TaskEventKind, TaskPatch};
pub(crate) use store::{
    ensure_default_project,
    get_project,
    list_buckets,
    list_tasks,
};
