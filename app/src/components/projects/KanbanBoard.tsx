import { DragDropContext, type DropResult } from '@hello-pangea/dnd';

import type { BoardData, Task } from '../../services/api/projectsApi';
import { KanbanColumn } from './KanbanColumn';

interface Props {
  board: BoardData;
  onTaskClick: (task: Task) => void;
  onAddTask: (
    bucketId: string,
    title: string,
    opts?: { assignee?: string; due_date?: string; priority?: number }
  ) => Promise<void>;
  onAddViaModal: (bucketId: string) => void;
  onMoveTask: (taskId: string, destBucketId: string, destIndex: number) => Promise<void>;
  onRenameColumn: (bucketId: string, title: string) => Promise<void>;
}

export function KanbanBoard({
  board,
  onTaskClick,
  onAddTask,
  onAddViaModal,
  onMoveTask,
  onRenameColumn,
}: Props) {
  const onDragEnd = async (result: DropResult) => {
    if (!result.destination) return;
    const { draggableId, destination } = result;
    if (
      result.source.droppableId === destination.droppableId &&
      result.source.index === destination.index
    )
      return;
    await onMoveTask(draggableId, destination.droppableId, destination.index);
  };

  return (
    <DragDropContext onDragEnd={result => void onDragEnd(result)}>
      <div className="flex gap-4 items-start h-full min-w-0">
        {board.buckets.map(({ bucket, tasks }) => (
          <KanbanColumn
            key={bucket.id}
            bucket={bucket}
            tasks={tasks}
            onTaskClick={onTaskClick}
            onAddTask={onAddTask}
            onAddViaModal={onAddViaModal}
            onRenameColumn={onRenameColumn}
          />
        ))}
      </div>
    </DragDropContext>
  );
}
