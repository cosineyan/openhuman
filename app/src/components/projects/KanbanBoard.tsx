import { DragDropContext, type DropResult } from '@hello-pangea/dnd';
import { KanbanColumn } from './KanbanColumn';
import type { BoardData, Task } from '../../services/api/projectsApi';

interface Props {
  board: BoardData;
  onTaskClick: (task: Task) => void;
  onAddTask: (bucketId: string, title: string) => Promise<void>;
  onMoveTask: (taskId: string, destBucketId: string, destIndex: number) => Promise<void>;
  onRenameColumn: (bucketId: string, title: string) => Promise<void>;
}

export function KanbanBoard({ board, onTaskClick, onAddTask, onMoveTask, onRenameColumn }: Props) {
  const onDragEnd = async (result: DropResult) => {
    if (!result.destination) return;
    const { draggableId, destination } = result;
    if (
      result.source.droppableId === destination.droppableId &&
      result.source.index === destination.index
    ) return;
    await onMoveTask(draggableId, destination.droppableId, destination.index);
  };

  return (
    <DragDropContext onDragEnd={result => void onDragEnd(result)}>
      <div className="flex gap-4 overflow-x-auto pb-4">
        {board.buckets.map(({ bucket, tasks }) => (
          <KanbanColumn
            key={bucket.id}
            bucket={bucket}
            tasks={tasks}
            onTaskClick={onTaskClick}
            onAddTask={onAddTask}
            onRenameColumn={onRenameColumn}
          />
        ))}
      </div>
    </DragDropContext>
  );
}
