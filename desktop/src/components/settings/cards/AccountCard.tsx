import { User } from '../../../lib/icons';
import { useAuthStore } from '../../../stores/auth';
import { Card } from '../primitives';

export function AccountCard() {
  const logout = useAuthStore((s) => s.logout);

  return (
    <Card title={'Account'} icon={<User size={17} />}>
      <div className="flex flex-col gap-2.5">
        <button
          type="button"
          onClick={logout}
          className="flex items-center gap-2 px-5 py-2.5 rounded-md text-[13px] font-semibold bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/10 hover:border-red-500/20 transition-all duration-300 cursor-pointer w-fit"
        >
          {'Sign Out'}
        </button>
      </div>
    </Card>
  );
}
