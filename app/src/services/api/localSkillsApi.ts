import { callCoreRpc } from '../coreRpcClient';

export interface LocalSkill {
  name: string;
  description: string;
  when_to_use: string | null;
  author: string | null;
  body: string;
  plugin_name: string;
  version: string;
}

export async function listLocalSkills(): Promise<LocalSkill[]> {
  const res = await callCoreRpc<{ result: LocalSkill[]; logs: string[] }>({
    method: 'openhuman.local_skills_list',
  });
  return res.result ?? [];
}
