import { useState } from 'react';
import { Droppable, Draggable } from '@hello-pangea/dnd';
import { KanbanCard } from './KanbanCard';
import { NewTaskInput } from './NewTaskInput';
import type { Bucket, Task } from '../../services/api/projectsApi';

interface Props {
  bucket: Bucket;
  tasks: Task[];
  onTaskClick: (task: Task) => void;
  onAddTask: (bucketId: string, title: string, opts?: { assignee?: string; due_date?: string; priority?: number }) => Promise<void>;
  onAddViaModal: (bucketId: string) => void;
  onRenameColumn: (bucketId: string, title: string) => Promise<void>;
}

type BucketStyle = {
  icon: React.ReactNode;
  badge: string;        // pill classes when tasks > 0
  addTaskColor: string; // color of "+ Add Task" text
  columnBg: string;     // column background
};

function getBucketStyle(title: string, isDone: boolean): BucketStyle {
  const t = title.toLowerCase();

  if (isDone || t.includes('done') || t.includes('complete')) {
    return {
      icon: (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="7" fill="#22c55e"/>
          <path d="M5 8l2.5 2.5L11 5.5" stroke="white" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      ),
      badge: 'bg-green-100 text-green-700 dark:bg-green-500/20 dark:text-green-400',
      addTaskColor: 'text-green-600 dark:text-green-500 hover:text-green-700',
      columnBg: 'bg-green-50/60 dark:bg-green-500/5',
    };
  }
  if (t.includes('progress') || t.includes('doing')) {
    return {
      icon: (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="7" fill="#4A83DD"/>
          <circle cx="8" cy="8" r="3.5" fill="white"/>
        </svg>
      ),
      badge: 'bg-blue-100 text-blue-700 dark:bg-blue-500/20 dark:text-blue-400',
      addTaskColor: 'text-primary-500 dark:text-primary-400 hover:text-primary-600',
      columnBg: 'bg-blue-50/60 dark:bg-blue-500/5',
    };
  }
  if (t.includes('block')) {
    return {
      icon: (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="6.5" stroke="#f87171" strokeWidth="1.5" strokeDasharray="3 2"/>
        </svg>
      ),
      badge: 'bg-red-100 text-red-600 dark:bg-red-500/20 dark:text-red-400',
      addTaskColor: 'text-red-400 hover:text-red-600',
      columnBg: 'bg-red-50/40 dark:bg-red-500/5',
    };
  }
  // To Do
  return {
    icon: (
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
        <circle cx="8" cy="8" r="6.5" stroke="#a8a29e" strokeWidth="1.5" strokeDasharray="3 2"/>
      </svg>
    ),
    badge: 'bg-stone-100 text-stone-600 dark:bg-neutral-700 dark:text-neutral-400',
    addTaskColor: 'text-stone-400 dark:text-neutral-500 hover:text-stone-600',
    columnBg: 'bg-stone-50 dark:bg-neutral-900/60',
  };
}

export function KanbanColumn({ bucket, tasks, onTaskClick, onAddTask, onAddViaModal, onRenameColumn }: Props) {
  const [editing, setEditing] = useState(false);
  const [titleDraft, setTitleDraft] = useState(bucket.title);
  const style = getBucketStyle(bucket.title, bucket.is_done_bucket);

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
    <div className={`flex flex-col flex-1 min-w-52 max-w-xs shrink-0 rounded-xl ${style.columnBg} px-3 pt-3 pb-2`}>
      {/* Column header */}
      <div className="flex items-center gap-2 mb-3">
        {style.icon}

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
            className="flex-1 rounded border border-primary-400 bg-white dark:bg-neutral-800 px-1.5 py-0.5 text-xs font-bold tracking-widest uppercase text-stone-800 dark:text-neutral-200 focus:outline-none"
          />
        ) : (
          <button
            type="button"
            onDoubleClick={() => setEditing(true)}
            title="Double-click to rename"
            className="flex-1 text-left text-xs font-bold tracking-widest uppercase text-stone-700 dark:text-neutral-300 truncate"
          >
            {bucket.title}
          </button>
        )}

        <span className={`text-xs font-semibold px-1.5 py-0.5 rounded-md ${style.badge}`}>
          {tasks.length}
        </span>

        <button type="button" title="Add task" onClick={() => onAddViaModal(bucket.id)}
          className="p-0.5 text-stone-400 dark:text-neutral-500 hover:text-stone-600 dark:hover:text-neutral-300 ml-1">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M8 3v10M3 8h10" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"/>
          </svg>
        </button>
      </div>

      {/* Cards */}
      <Droppable droppableId={bucket.id}>
        {(provided, snapshot) => (
          <div
            ref={provided.innerRef}
            {...provided.droppableProps}
            className={`flex flex-col gap-2 min-h-[2rem] rounded-lg transition-colors ${snapshot.isDraggingOver ? 'bg-primary-100/50 dark:bg-primary-500/10' : ''}`}
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

      {/* Add task — inline form or button */}
      <div className="mt-2">
        <NewTaskInput
          addTaskColor={style.addTaskColor}
          onAdd={(title, opts) => onAddTask(bucket.id, title, opts)}
        />
      </div>
    </div>
  );
}
