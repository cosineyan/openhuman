import { useState } from 'react';
import { Droppable, Draggable } from '@hello-pangea/dnd';
import { KanbanCard } from './KanbanCard';
import { NewTaskInput } from './NewTaskInput';
import type { Bucket, Task } from '../../services/api/projectsApi';

interface Props {
  bucket: Bucket;
  tasks: Task[];
  onTaskClick: (task: Task) => void;
  onAddTask: (bucketId: string, title: string) => Promise<void>;
  onRenameColumn: (bucketId: string, title: string) => Promise<void>;
}

export function KanbanColumn({ bucket, tasks, onTaskClick, onAddTask, onRenameColumn }: Props) {
  const [editing, setEditing] = useState(false);
  const [titleDraft, setTitleDraft] = useState(bucket.title);

  const commitRename = async () => {
    const trimmed = titleDraft.trim();
    if (trimmed && trimmed !== bucket.title) {
      await onRenameColumn(bucket.id, trimmed);
    } else {
      setTitleDraft(bucket.title);
    }
    setEditing(false);
  };

  return (
    <div className="flex flex-col w-64 shrink-0 rounded-xl border border-stone-200 dark:border-neutral-800 bg-stone-50 dark:bg-neutral-900 p-3 gap-2">
      <div className="flex items-center justify-between gap-2 pb-1">
        {editing ? (
          <input
            autoFocus
            value={titleDraft}
            onChange={e => setTitleDraft(e.target.value)}
            onBlur={() => void commitRename()}
            onKeyDown={e => {
              if (e.key === 'Enter') void commitRename();
              if (e.key === 'Escape') { setTitleDraft(bucket.title); setEditing(false); }
            }}
            className="flex-1 rounded border border-primary-400 bg-white dark:bg-neutral-800 px-2 py-0.5 text-sm font-semibold text-stone-900 dark:text-neutral-100 focus:outline-none"
          />
        ) : (
          <button
            type="button"
            onDoubleClick={() => setEditing(true)}
            title="Double-click to rename"
            className="flex-1 text-left text-sm font-semibold text-stone-700 dark:text-neutral-200 truncate"
          >
            {bucket.title}
          </button>
        )}
        <span className="text-xs text-stone-400 dark:text-neutral-500 shrink-0">
          {tasks.length}
        </span>
      </div>

      <Droppable droppableId={bucket.id}>
        {(provided, snapshot) => (
          <div
            ref={provided.innerRef}
            {...provided.droppableProps}
            className={`flex flex-col gap-2 min-h-[4rem] rounded-lg transition-colors ${snapshot.isDraggingOver ? 'bg-primary-50 dark:bg-primary-500/10' : ''}`}
          >
            {tasks.map((task, index) => (
              <Draggable key={task.id} draggableId={task.id} index={index}>
                {(dragProvided, dragSnapshot) => (
                  <div
                    ref={dragProvided.innerRef}
                    {...dragProvided.draggableProps}
                    {...dragProvided.dragHandleProps}
                    className={dragSnapshot.isDragging ? 'opacity-80 rotate-1' : ''}
                  >
                    <KanbanCard task={task} onClick={onTaskClick} />
                  </div>
                )}
              </Draggable>
            ))}
            {provided.placeholder}
          </div>
        )}
      </Droppable>

      <NewTaskInput onAdd={title => onAddTask(bucket.id, title)} />
    </div>
  );
}
