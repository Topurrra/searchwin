'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const shield = require('../youtube-shield.js');

// ---- pruneAdFields -------------------------------------------------------

test('pruneAdFields removes known ad fields but keeps everything else', () => {
  const playerResponse = {
    playabilityStatus: { status: 'OK' },
    streamingData: { formats: [], adaptiveFormats: [{ itag: 137 }] },
    videoDetails: { videoId: 'abc123', title: 'A real video' },
    adPlacements: [{ adPlacementRenderer: {} }],
    playerAds: [{ playerLegacyDesktopWatchAdsRenderer: {} }],
    adSlots: [{ id: 1 }],
    adBreakHeartbeatParams: 'opaque-token',
  };

  shield.pruneAdFields(playerResponse);

  assert.equal(playerResponse.adPlacements, undefined);
  assert.equal(playerResponse.playerAds, undefined);
  assert.equal(playerResponse.adSlots, undefined);
  assert.equal(playerResponse.adBreakHeartbeatParams, undefined);
  // Playback-critical data survives untouched.
  assert.deepEqual(playerResponse.streamingData.adaptiveFormats, [{ itag: 137 }]);
  assert.equal(playerResponse.videoDetails.videoId, 'abc123');
  assert.equal(playerResponse.playabilityStatus.status, 'OK');
});

test('pruneAdFields never touches a field whose name merely starts with "ad"', () => {
  const obj = { adaptiveFormats: [1, 2, 3], address: 'keep me', additionalInfo: 42 };
  shield.pruneAdFields(obj);
  assert.deepEqual(obj, { adaptiveFormats: [1, 2, 3], address: 'keep me', additionalInfo: 42 });
});

test('pruneAdFields finds ad fields nested inside arrays', () => {
  const obj = { contents: [{ irrelevant: 1 }, { adSlots: [1, 2] }] };
  shield.pruneAdFields(obj);
  assert.equal(obj.contents[1].adSlots, undefined);
  assert.deepEqual(obj.contents[0], { irrelevant: 1 });
});

test('pruneAdFields tolerates a circular structure without hanging or throwing', () => {
  const obj = { adPlacements: [1] };
  obj.self = obj;
  assert.doesNotThrow(() => shield.pruneAdFields(obj));
  assert.equal(obj.adPlacements, undefined);
  assert.equal(obj.self, obj);
});

test('pruneAdFields is a no-op on primitives and null', () => {
  assert.equal(shield.pruneAdFields(null), null);
  assert.equal(shield.pruneAdFields(42), 42);
  assert.equal(shield.pruneAdFields('x'), 'x');
  assert.equal(shield.pruneAdFields(undefined), undefined);
});

// ---- isPlayerResponseShape ------------------------------------------------

test('isPlayerResponseShape recognises real player-response shapes', () => {
  assert.equal(shield.isPlayerResponseShape({ playabilityStatus: {} }), true);
  assert.equal(shield.isPlayerResponseShape({ streamingData: {} }), true);
  assert.equal(shield.isPlayerResponseShape({ videoDetails: {} }), true);
  assert.equal(shield.isPlayerResponseShape({ adPlacements: [] }), true);
});

test('isPlayerResponseShape rejects ordinary data', () => {
  assert.equal(shield.isPlayerResponseShape({ hello: 'world' }), false);
  assert.equal(shield.isPlayerResponseShape([1, 2, 3]), false);
  assert.equal(shield.isPlayerResponseShape('a string'), false);
  assert.equal(shield.isPlayerResponseShape(null), false);
});

// ---- pruneJsonText ----------------------------------------------------

test('pruneJsonText prunes JSON text that looks like player data', () => {
  const text = JSON.stringify({ videoDetails: { videoId: 'x' }, adPlacements: [1] });
  const result = shield.pruneJsonText(text);
  const parsed = JSON.parse(result);
  assert.equal(parsed.adPlacements, undefined);
  assert.equal(parsed.videoDetails.videoId, 'x');
});

test('pruneJsonText returns non-player JSON completely unchanged', () => {
  const text = JSON.stringify({ hello: 'world', nested: { count: 3 } });
  assert.equal(shield.pruneJsonText(text), text);
});

test('pruneJsonText returns invalid JSON unchanged instead of throwing', () => {
  const text = '{ not valid json';
  assert.equal(shield.pruneJsonText(text), text);
});

test('pruneJsonText passes through non-string input unchanged', () => {
  assert.equal(shield.pruneJsonText(undefined), undefined);
  assert.equal(shield.pruneJsonText(null), null);
});

// ---- installGlobalTrap --------------------------------------------------

test('installGlobalTrap prunes a value assigned later, as if by `var x = {...}`', () => {
  const fakeWindow = {};
  shield.installGlobalTrap(fakeWindow, 'ytInitialPlayerResponse');
  fakeWindow.ytInitialPlayerResponse = { videoDetails: { videoId: 'x' }, adPlacements: [1] };
  assert.equal(fakeWindow.ytInitialPlayerResponse.adPlacements, undefined);
  assert.equal(fakeWindow.ytInitialPlayerResponse.videoDetails.videoId, 'x');
});

test('installGlobalTrap prunes a value that was already set before the trap was installed', () => {
  const fakeWindow = { ytInitialData: { adSlots: [1], contents: { real: true } } };
  shield.installGlobalTrap(fakeWindow, 'ytInitialData');
  assert.equal(fakeWindow.ytInitialData.adSlots, undefined);
  assert.equal(fakeWindow.ytInitialData.contents.real, true);
});

test('installGlobalTrap leaves a non-configurable existing property alone rather than throwing', () => {
  const fakeWindow = {};
  Object.defineProperty(fakeWindow, 'ytInitialData', { value: { adSlots: [1] }, configurable: false });
  assert.doesNotThrow(() => shield.installGlobalTrap(fakeWindow, 'ytInitialData'));
  assert.equal(fakeWindow.ytInitialData.adSlots.length, 1); // untouched, but did not throw
});

// ---- patchJsonParse -----------------------------------------------------

test('patchJsonParse prunes only player-shaped JSON.parse results', () => {
  const fakeJson = { parse: JSON.parse };
  shield.patchJsonParse(fakeJson);

  const player = fakeJson.parse(JSON.stringify({ streamingData: {}, playerAds: [1] }));
  assert.equal(player.playerAds, undefined);

  const ordinary = fakeJson.parse(JSON.stringify({ foo: 'bar' }));
  assert.deepEqual(ordinary, { foo: 'bar' });
});

test('patchJsonParse only patches once', () => {
  const fakeJson = { parse: JSON.parse };
  shield.patchJsonParse(fakeJson);
  const patchedOnce = fakeJson.parse;
  const appliedAgain = shield.patchJsonParse(fakeJson);
  assert.equal(appliedAgain, false);
  assert.equal(fakeJson.parse, patchedOnce);
});

// ---- patchFetch -----------------------------------------------------------

test('patchFetch prunes the body of a matching youtubei endpoint', async () => {
  const rawBody = JSON.stringify({ videoDetails: { videoId: 'x' }, adPlacements: [1] });
  const fakeWindow = {
    Response: Response,
    fetch: async () => new Response(rawBody, { status: 200 }),
  };
  shield.patchFetch(fakeWindow);

  const response = await fakeWindow.fetch('https://www.youtube.com/youtubei/v1/player?key=1');
  const text = await response.text();
  const parsed = JSON.parse(text);
  assert.equal(parsed.adPlacements, undefined);
  assert.equal(parsed.videoDetails.videoId, 'x');
});

test('patchFetch leaves non-matching URLs completely untouched', async () => {
  const rawBody = JSON.stringify({ adPlacements: [1] });
  let calledWith = null;
  const fakeWindow = {
    Response: Response,
    fetch: async (url) => { calledWith = url; return new Response(rawBody, { status: 200 }); },
  };
  shield.patchFetch(fakeWindow);

  const response = await fakeWindow.fetch('https://www.youtube.com/watch?v=x');
  const text = await response.text();
  assert.equal(calledWith, 'https://www.youtube.com/watch?v=x');
  assert.equal(text, rawBody); // untouched, ad field and all
});

test('patchFetch only patches once', () => {
  const original = async () => new Response('{}');
  const fakeWindow = { Response: Response, fetch: original };
  shield.patchFetch(fakeWindow);
  const patchedOnce = fakeWindow.fetch;
  const appliedAgain = shield.patchFetch(fakeWindow);
  assert.equal(appliedAgain, false);
  assert.equal(fakeWindow.fetch, patchedOnce);
});

// ---- patchXhr -------------------------------------------------------------

function makeFakeXhrCtor(responseText) {
  function FakeXHR() {}
  FakeXHR.prototype.open = function (method, url) { this._url = url; };
  Object.defineProperty(FakeXHR.prototype, 'responseText', {
    configurable: true,
    get: function () { return responseText; },
  });
  return FakeXHR;
}

test('patchXhr prunes responseText only for a matching URL', () => {
  const rawBody = JSON.stringify({ videoDetails: {}, playerAds: [1] });
  const FakeXHR = makeFakeXhrCtor(rawBody);
  shield.patchXhr(FakeXHR);

  const matching = new FakeXHR();
  matching.open('POST', 'https://www.youtube.com/youtubei/v1/next');
  const parsed = JSON.parse(matching.responseText);
  assert.equal(parsed.playerAds, undefined);

  const other = new FakeXHR();
  other.open('GET', 'https://www.youtube.com/somethingelse');
  assert.equal(other.responseText, rawBody); // untouched
});

test('patchXhr only patches a given constructor once', () => {
  const FakeXHR = makeFakeXhrCtor('{}');
  assert.equal(shield.patchXhr(FakeXHR), true);
  assert.equal(shield.patchXhr(FakeXHR), false);
});

// ---- init: never throws, even with a hostile/missing window --------------

test('init never throws when handed nothing at all', () => {
  assert.doesNotThrow(() => shield.init(undefined));
});

test('init skips everything on a non-YouTube hostname', () => {
  const fakeWindow = { location: { hostname: 'example.com' } };
  shield.init(fakeWindow);
  assert.equal(Object.prototype.hasOwnProperty.call(fakeWindow, 'ytInitialData'), false);
});

test('init wires the traps on a youtube.com hostname without throwing', () => {
  const fakeDoc = { createElement: () => ({ setAttribute() {}, }), head: { appendChild() {} } };
  const fakeWindow = { location: { hostname: 'www.youtube.com' }, document: fakeDoc, JSON: { parse: JSON.parse } };
  assert.doesNotThrow(() => shield.init(fakeWindow));
  fakeWindow.ytInitialData = { adSlots: [1] };
  assert.equal(fakeWindow.ytInitialData.adSlots, undefined);
});
