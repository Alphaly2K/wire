/** Stereo signed PCM consumer backed by Web Audio's scheduled buffer sources. */
export async function createAudioSink({context} = {}) {
  const owned = context === undefined;
  if (owned) {
    if (typeof globalThis.AudioContext !== 'function') throw new Error('Web Audio output is unavailable');
    context = new globalThis.AudioContext({sampleRate:44100, latencyHint:'interactive'});
  }
  const sources = new Map();
  let next = 0, closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    for (const source of sources.keys()) { try { source.stop(); } catch {} source.disconnect(); }
    sources.clear();
    if (owned) context.close().catch(() => {});
  };
  try {
    if (!context || typeof context.createBuffer !== 'function' || typeof context.createBufferSource !== 'function' || !context.destination || context.state === 'closed') throw new TypeError('expected an available Web Audio context');
    if (owned) { await context.resume(); if (context.state !== 'running') throw new Error('Web Audio output could not start'); }
    return {
      write(samples) {
        if (closed) throw new Error('audio output has been closed');
        if (!(samples instanceof Int16Array) || samples.length % 2 || samples.length > 88200) throw new TypeError('expected bounded stereo signed PCM');
        if (!samples.length) return true;
        if (context.state !== 'running' && typeof context.startRendering !== 'function') return false;
        const now = context.currentTime;
        for (const [source,end] of sources) if (end <= now) { source.disconnect(); sources.delete(source); }
        const frames = samples.length/2, start = Math.max(now+0.01,next), end = start+frames/44100;
        if (end > now+2) return false;
        const buffer = context.createBuffer(2,frames,44100);
        for (let channel=0;channel<2;channel++) {
          const output = buffer.getChannelData(channel);
          for (let frame=0;frame<frames;frame++) output[frame]=samples[frame*2+channel]/32768;
        }
        const source = context.createBufferSource();
        source.buffer=buffer;
        source.connect(context.destination);
        source.onended=()=>{sources.delete(source);source.disconnect();};
        try { source.start(start); }
        catch (error) { source.disconnect(); throw error; }
        sources.set(source,end);next=end;
        return true;
      },
      close,
    };
  } catch (error) { close(); throw error; }
}
