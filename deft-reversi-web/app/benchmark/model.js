/** Compare exact solutions only, since fixed-depth estimates are not final scores. */
export function compareAnswer(result, answers) {
    if (!result.solved || !answers?.length) return null;
    const best = Math.max(...answers.map(answer => answer.score));
    return result.score === best && answers.some(answer => answer.score === best && answer.move === result.bestMove);
}

export function formatDuration(ms) {
    return ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${Math.round(ms)} ms`;
}

export function nodesPerSecond(nodes, ms) {
    if (!(ms > 0)) return '—';
    const n = BigInt(nodes);
    const micros = BigInt(Math.max(1, Math.round(ms * 1000)));
    return ((n * 1000000n) / micros).toLocaleString('en-US');
}
