export function Placeholder({ title, phase }: { title: string; phase: string }) {
  return (
    <>
      <h1 className="mb-2 text-xl font-semibold">{title}</h1>
      <p className="text-sm text-neutral-500">Coming in {phase} of the roadmap.</p>
    </>
  );
}
