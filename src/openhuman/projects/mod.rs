mod ops;
mod schemas;
pub(crate) mod store;
mod types;
pub mod tools; // placeholder — Task 4 fills this in

pub use ops::{
    create_task, delete_task, get_board, move_task, update_bucket, update_task,
    BucketWithTasks, BucketsWithTasks, CreateTaskInput,
};
pub use schemas::{
    all_controller_schemas as all_projects_controller_schemas,
    all_registered_controllers as all_projects_registered_controllers,
};
pub use types::{Bucket, BucketPatch, Project, Task, TaskPatch};
pub(crate) use store::{
    create_task as store_create_task,
    delete_task as store_delete_task,
    ensure_default_project,
    get_project,
    list_buckets,
    list_tasks,
    update_bucket as store_update_bucket,
    update_task as store_update_task,
};
