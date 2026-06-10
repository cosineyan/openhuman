import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { socketService } from '../../services/socketService';
import { useAiTaskRuns } from './useAiTaskRuns';

vi.mock('../../services/socketService', () => ({ socketService: { on: vi.fn(), off: vi.fn() } }));

vi.mock('../../services/api/projectsApi', () => ({
  listRunningAiTasks: vi.fn().mockResolvedValue({ task_ids: ['task-existing'] }),
}));

describe('useAiTaskRuns', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('seeds running state from listRunningAiTasks on mount', async () => {
    const { result } = renderHook(() => useAiTaskRuns());
    await waitFor(() => {
      expect(result.current.isRunning('task-existing')).toBe(true);
    });
  });

  it('registers and deregisters socket listener', () => {
    const { unmount } = renderHook(() => useAiTaskRuns());
    expect(socketService.on).toHaveBeenCalledWith('project:task_log', expect.any(Function));
    unmount();
    expect(socketService.off).toHaveBeenCalledWith('project:task_log', expect.any(Function));
  });

  it('adds log line and marks running on log event', async () => {
    const { result } = renderHook(() => useAiTaskRuns());

    const listener = (socketService.on as ReturnType<typeof vi.fn>).mock.calls.find(
      ([event]: [string]) => event === 'project:task_log'
    )?.[1] as ((data: unknown) => void) | undefined;

    act(() => {
      listener?.({ output: JSON.stringify({ task_id: 'task-new', line: 'hello', kind: 'log' }) });
    });

    expect(result.current.isRunning('task-new')).toBe(true);
    expect(result.current.getLines('task-new')).toEqual(['hello']);
  });

  it('marks done on terminal event', async () => {
    const { result } = renderHook(() => useAiTaskRuns());

    const listener = (socketService.on as ReturnType<typeof vi.fn>).mock.calls.find(
      ([event]: [string]) => event === 'project:task_log'
    )?.[1] as ((data: unknown) => void) | undefined;

    act(() => {
      listener?.({ output: JSON.stringify({ task_id: 'task-fin', line: 'Done!', kind: 'done' }) });
    });

    expect(result.current.isRunning('task-fin')).toBe(false);
    expect(result.current.getRun('task-fin')?.status).toBe('done');
  });
});
