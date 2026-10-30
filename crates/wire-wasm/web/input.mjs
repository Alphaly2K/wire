// DOM physical codes map to Windows VK and set-1 scan codes; text comes from KeyboardEvent.key.
const keys = new Map();
const add = (code, vk, scan, extended = false) => keys.set(code, {vk, scan, extended});
const letters = [0x1e,0x30,0x2e,0x20,0x12,0x21,0x22,0x23,0x17,0x24,0x25,0x26,0x32,0x31,0x18,0x19,0x10,0x13,0x1f,0x14,0x16,0x2f,0x11,0x2d,0x15,0x2c];
for (let n=0;n<26;n++) add('Key'+String.fromCharCode(65+n),65+n,letters[n]);
for (let n=0;n<10;n++) add('Digit'+n,48+n,n===0?11:n+1);
for (let n=1;n<=12;n++) add('F'+n,111+n,n<=10?58+n:76+n);
for (const [code,vk,scan,extended] of [
  ['Escape',27,1],['Backspace',8,14],['Tab',9,15],['Enter',13,28],['Space',32,57],
  ['ShiftLeft',16,42],['ShiftRight',16,54],['ControlLeft',17,29],['ControlRight',17,29,true],
  ['AltLeft',18,56],['AltRight',18,56,true],['MetaLeft',91,91,true],['MetaRight',92,92,true],['ContextMenu',93,93,true],
  ['CapsLock',20,58],['NumLock',144,69,true],['ScrollLock',145,70],['PrintScreen',44,55,true],['Pause',19,69],
  ['Home',36,71,true],['End',35,79,true],['PageUp',33,73,true],['PageDown',34,81,true],
  ['Insert',45,82,true],['Delete',46,83,true],['ArrowLeft',37,75,true],['ArrowUp',38,72,true],['ArrowRight',39,77,true],['ArrowDown',40,80,true],
  ['Semicolon',186,39],['Equal',187,13],['Comma',188,51],['Minus',189,12],['Period',190,52],['Slash',191,53],
  ['Backquote',192,41],['BracketLeft',219,26],['Backslash',220,43],['BracketRight',221,27],['Quote',222,40],['IntlBackslash',226,86],
  ['NumpadMultiply',106,55],['NumpadAdd',107,78],['NumpadSubtract',109,74],['NumpadDecimal',110,83],['NumpadDivide',111,53,true],['NumpadEnter',13,28,true],
]) add(code,vk,scan,extended);
for (const [n,scan] of [82,79,80,81,75,76,77,71,72,73].entries()) add('Numpad'+n,96+n,scan);

export function physicalKey(code) { return keys.get(code); }
export function keyParameter(key, {down, previous = false, alt = false}) {
  return BigInt((1 | key.scan<<16 | Number(key.extended)<<24 | Number(alt)<<29 |
    Number(previous || !down)<<30 | Number(!down)<<31) >>> 0);
}
export function keyText(event) {
  if (event.isComposing || event.ctrlKey || event.metaKey) return '';
  if (typeof event.key === 'string' && [...event.key].length === 1) return event.key;
  return {Enter:'\r',Tab:'\t',Backspace:'\b'}[event.key] ?? '';
}
export const mouseFlags = (buttons, event) => buttons | (event.shiftKey?4:0) | (event.ctrlKey?8:0);
export const packedPoint = (x,y) => BigInt(((x&65535) | (y&65535)<<16) >>> 0);
