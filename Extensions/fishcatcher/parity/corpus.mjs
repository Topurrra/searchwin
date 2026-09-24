// The parity corpus: addresses and page facts that both the original engine
// (golden.mjs, over src/engine) and Search's C# port (SearchKit.FishCatcher)
// score, so the two can be compared verdict for verdict. Deterministic: no
// clocks, no Math.random, the same list on every run.

// Popular, real sites (and their usual sign-in pages).
const LEGIT = [
  'https://www.google.com/', 'https://google.com/search?q=cats', 'https://mail.google.com/mail/u/0/',
  'https://accounts.google.com/signin', 'https://docs.google.com/document/d/1', 'https://drive.google.com/',
  'https://www.youtube.com/watch?v=dQw4w9WgXcQ', 'https://www.facebook.com/login', 'https://m.facebook.com/',
  'https://www.amazon.com/', 'https://www.amazon.co.uk/gp/cart', 'https://www.amazon.de/', 'https://aws.amazon.com/',
  'https://console.aws.amazon.com/console/home', 'https://signin.aws.amazon.com/', 'https://en.wikipedia.org/wiki/Phishing',
  'https://twitter.com/home', 'https://x.com/', 'https://www.instagram.com/accounts/login/', 'https://www.linkedin.com/feed/',
  'https://www.reddit.com/r/programming/', 'https://www.netflix.com/login', 'https://www.microsoft.com/en-us/',
  'https://login.microsoftonline.com/common/oauth2/authorize', 'https://outlook.live.com/mail/', 'https://outlook.office.com/',
  'https://www.office.com/', 'https://contoso.sharepoint.com/sites/team', 'https://www.apple.com/', 'https://appleid.apple.com/',
  'https://www.icloud.com/', 'https://github.com/login', 'https://gist.github.com/', 'https://someone.github.io/project/',
  'https://stackoverflow.com/questions', 'https://www.paypal.com/signin', 'https://www.bankofamerica.com/',
  'https://secure.chase.com/web/auth/', 'https://www.wellsfargo.com/', 'https://www.bbc.co.uk/news', 'https://www.bbc.com/',
  'https://www.nytimes.com/', 'https://edition.cnn.com/', 'https://www.theguardian.com/', 'https://www.ft.com/',
  'https://go.com/', 'https://www.box.com/', 'https://goo.gl/maps', 'https://www.redhat.com/', 'https://www.google.ca/',
  'https://www.vanguard.ca/', 'https://protonmail.ch/', 'https://proton.me/mail', 'https://www.yahoo.com/',
  'https://www.bing.com/', 'https://duckduckgo.com/', 'https://zoom.us/signin', 'https://www.dropbox.com/login',
  'https://www.adobe.com/', 'https://open.spotify.com/', 'https://www.twitch.tv/', 'https://discord.com/login',
  'https://web.whatsapp.com/', 'https://t.me/durov', 'https://telegram.org/', 'https://www.ebay.com/', 'https://www.walmart.com/',
  'https://www.target.com/', 'https://www.bestbuy.com/', 'https://www.booking.com/', 'https://www.airbnb.com/',
  'https://www.expedia.com/', 'https://www.dhl.com/', 'https://www.fedex.com/en-us/tracking.html', 'https://www.ups.com/',
  'https://tools.usps.com/go/TrackConfirmAction', 'https://www.royalmail.com/', 'https://www.irs.gov/', 'https://www.gov.uk/',
  'https://tbcbank.ge/', 'https://bankofgeorgia.ge/', 'https://www.coinbase.com/signin', 'https://www.binance.com/en',
  'https://www.kraken.com/', 'https://metamask.io/', 'https://www.ledger.com/', 'https://www.cloudflare.com/',
  'https://mybucket.s3.amazonaws.com/file.txt', 'https://storage.googleapis.com/bucket/object', 'https://my-app.vercel.app/',
  'https://my-site.netlify.app/', 'https://some-app.herokuapp.com/', 'https://someblog.blogspot.com/', 'https://someone.wordpress.com/',
  'https://medium.com/@someone/post', 'https://someone.substack.com/', 'https://www.notion.so/', 'https://app.slack.com/client',
  'https://team.atlassian.net/jira', 'https://login.salesforce.com/', 'https://example.okta.com/app', 'https://openai.com/',
  'https://www.anthropic.com/', 'https://claude.ai/new', 'https://www.npmjs.com/package/left-pad', 'https://pypi.org/project/requests/',
  'https://www.nuget.org/', 'https://crates.io/', 'https://www.mozilla.org/', 'https://developer.mozilla.org/en-US/docs/Web',
  'https://www.rust-lang.org/', 'https://www.python.org/', 'https://yandex.ru/', 'https://vk.com/', 'https://mail.ru/',
  'https://www.baidu.com/', 'https://www.qq.com/', 'https://www.taobao.com/', 'https://www.aliexpress.com/',
  'https://www.rakuten.co.jp/', 'https://www.naver.com/', 'https://www.samsung.com/', 'https://www.sony.com/',
  'https://www.nike.com/', 'https://www.ikea.com/', 'https://store.steampowered.com/', 'https://steamcommunity.com/',
  'https://www.roblox.com/login', 'https://www.hotmail.com/', 'https://www.outlook.com/', 'https://live.com/',
  'http://neverssl.com/', 'http://example.com/', 'https://example.org/', 'https://www.w3.org/', 'https://www.ietf.org/rfc/rfc3986.txt'
];

// Brand domains the look-alike generator starts from.
const BRAND_DOMAINS = [
  'paypal.com', 'apple.com', 'amazon.com', 'microsoft.com', 'google.com', 'facebook.com', 'netflix.com', 'chase.com',
  'wellsfargo.com', 'coinbase.com', 'binance.com', 'instagram.com', 'linkedin.com', 'dropbox.com', 'github.com',
  'dhl.com', 'fedex.com', 'usps.com', 'bankofamerica.com', 'steamcommunity.com', 'roblox.com', 'metamask.io',
  'outlook.com', 'office.com', 'icloud.com', 'whatsapp.com', 'tbcbank.ge', 'adobe.com', 'ebay.com', 'walmart.com'
];

const DIGIT_FOR = { o: '0', l: '1', i: '1', e: '3', a: '4', s: '5', t: '7', b: '8' };

function lookalikes(domain) {
  const dot = domain.indexOf('.');
  const name = domain.slice(0, dot);
  const tld = domain.slice(dot);
  const out = [];
  out.push(name.slice(0, 1) + name.slice(2) + tld);                             // a letter dropped
  out.push(name[0] + name[2] + name[1] + name.slice(3) + tld);                 // two letters swapped
  out.push(name.slice(0, 3) + name[2] + name.slice(3) + tld);                  // a letter doubled
  const i = [...name].findIndex((c) => DIGIT_FOR[c]);
  if (i >= 0) out.push(name.slice(0, i) + DIGIT_FOR[name[i]] + name.slice(i + 1) + tld); // a digit for a letter
  out.push(name + (tld === '.com' ? '.co' : '.com'));                          // another ending
  out.push(name + '-login' + tld);                                             // brand + keyword
  return out.map((d, n) => (n % 2 ? 'http://' : 'https://') + d + (n % 3 === 0 ? '/' : '/signin?next=%2Faccount'));
}

// Look-alike characters: Cyrillic/Greek letters that read as Latin ones.
const HOMOGLYPH = [
  'https://аpple.com/', 'https://раураl.com/signin', 'https://gооgle.com/', 'https://microsоft.com/', 'https://facebооk.com/',
  'https://аmazon.com/', 'https://netfliх.com/', 'https://αpple.com/', 'https://paypal-ѕecure.com/', 'https://xn--pple-43d.com/',
  'https://xn--80ak6aa92e.com/', 'https://www.xn--80ak6aa92e.com/login', 'https://іnstagram.com/', 'https://lіnkedin.com/',
  'https://ԁropbox.com/', 'https://coinbаse.com/', 'https://binаnce.com/', 'https://gіthub.com/', 'https://chаse.com/',
  'https://ebаy.com/', 'https://secure.раураl.com.xn--80ak6aa92e.com/', 'https://münchen.de/', 'https://bücher.de/',
  'https://日本.jp/', 'https://пример.рф/', 'https://ελληνικά.gr/', 'https://ｇｏｏｇｌｅ.com/', 'https://straße.de/',
  'https://i❤.ws/', 'https://ΑΡΡLE.com/', 'https://xn--mnchen-3ya.de/', 'https://XN--PPLE-43D.COM/', 'https://xn--zz.com/',
  'https://xn--abc-.com/', 'https://аррӏе.com/', 'https://wеllsfargo.com/', 'https://օutlook.com/', 'https://ⅰcloud.com/'
];

// A brand's name with a scare word, on a domain the brand doesn't own.
const BRAND_KEYWORD = [
  'https://paypal-login-verify.com/', 'https://secure-paypal.com.account-update.xyz/', 'https://appleid.apple.com.verify-account.tk/',
  'https://login-microsoftonline.com-session.info/', 'https://amazon-security-alert.top/', 'https://netflix-billing-update.click/',
  'https://chase-online-banking.support/', 'https://wellsfargo-secure.info/login', 'https://coinbase-wallet-sync.com/',
  'https://paypal-verify.com/', 'https://sub.paypal-verify.com/x', 'https://metamask-io-wallet.web.app/', 'https://dhl-parcel-tracking.online/',
  'https://usps-redelivery-fee.top/', 'https://office365-login.sharepoint-docs.site/', 'https://microsoft-teams-download.site/',
  'https://amaz0n-verify.com/', 'https://paypa1-secure.net/', 'https://g00gle-login.xyz/', 'https://faceb00k-security.ml/',
  'https://bog-ge-online.tk/', 'https://tbc-bank-login.ge/', 'https://netflix.account-hold.com/', 'https://steam-trade-offer.ru/',
  'https://www.instagram-copyright-appeal.ml/', 'https://icloud-find-my-iphone.com/', 'https://binance-support-center.help/',
  'https://ledger-live-update.app/', 'https://trezor-suite-sync.com/', 'https://royal-mail-redelivery.top/', 'https://walletconnect-fix.net/',
  'https://secure-login.bank-alert.com/', 'https://verify.account.example-support.com/', 'https://docusign-invoice.review/',
  'https://adobe-sign-document.online/', 'https://linkedin-job-offer.xyz/', 'https://outlook-mailbox-full.web.app/',
  'https://github-security-alert.pages.dev/', 'https://roblox-free-robux.com/', 'https://whatsapp-verify-code.site/'
];

// Numbers instead of a name, in every notation the address parser accepts.
const IP = [
  'http://192.0.2.1/login', 'http://203.0.113.45:8080/paypal/', 'https://198.51.100.7/', 'http://3232235777/', 'http://0x7f.1/',
  'http://8.8.8.8/', 'http://[2001:db8::1]/login', 'http://[::1]:3000/', 'http://10.0.0.5/', 'http://172.16.5.4/admin',
  'http://169.254.1.1/', 'http://0.0.0.0/', 'http://[fe80::1]/', 'http://1.1.1.1/', 'http://45.33.32.156/secure-login/',
  'http://0300.0250.0.1/', 'http://1.2.3/', 'http://[::ffff:1.2.3.4]/', 'http://[2001:0db8:0000:0000:0000:ff00:0042:8329]/',
  'http://172.32.0.1/', 'http://192.169.1.1/', 'http://127.0.0.1:8080/', 'https://[fd12:3456:789a::1]/', 'http://100.64.0.1/'
];

// High-abuse endings.
const RISKY_TLD = [
  'random-name.tk', 'freegift.ml', 'promo.ga', 'claim.cf', 'bonus.gq', 'shop.xyz', 'deals.top', 'verify.icu', 'go.click', 'news.buzz',
  'x.rest', 'invoice.zip', 'video.mov', 'outer.space', 'store.online', 'my.site', 'the.website', 'web.cam', 'a1.cfd', 'b2.sbs',
  'big.monster', 'q.quest', 'ha.lol', 'pw.pw', 'cc.cc', 'ws.ws', 'about.info', 'small.biz', 'night.club', 'stream.live',
  'news.today', 'hello.world', 'plain.com', 'plain.net'
].map((d, n) => (n % 4 === 3 ? 'http://' : 'https://') + d + '/');

// Long chains of names in front of the real domain.
const DEEP = [
  'https://login.microsoft.com.evil.xyz/', 'https://www.paypal.com.secure.login.verify.account.example.net/',
  'https://a.b.c.d.e.f.example.com/', 'https://secure.bankofamerica.com.session-id-8392.online/', 'https://www.google.co.uk/',
  'https://accounts.google.com.signin.v2.challenge.pwd.xyz/', 'https://a.b.github.io/', 'https://x.y.z.blogspot.com/',
  'https://foo.bar.kobe.jp/', 'https://city.kobe.jp/', 'https://www.ck/', 'https://foo.www.ck/', 'https://bar.foo.ck/',
  'https://co.uk/', 'https://foo.s3.amazonaws.com/', 'https://one.two.three.four.five.six.seven.eight.com/',
  'https://mail.yahoo.com.mail-login.accounts.verify-user.tk/', 'https://docs.google.com.shared-files.download.icu/',
  'https://www.amazon.co.jp/', 'https://shop.example.co.uk/', 'https://api.eu.example.com.au/', 'https://ab.cd.ef.gh/',
  'https://login.live.com.xn--80ak6aa92e.com/', 'https://secure.example.com.br/'
];

// Letter soup and digit-heavy names from a fixed-seed generator (mulberry32).
function mulberry32(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
function randomHosts(count) {
  const rnd = mulberry32(20260925);
  const letters = 'abcdefghijklmnopqrstuvwxyz';
  const mixed = 'abcdefghijklmnopqrstuvwxyz0123456789';
  const tlds = ['com', 'net', 'xyz', 'top', 'info', 'ru', 'cn', 'org', 'io', 'ge'];
  const out = [];
  for (let n = 0; n < count; n++) {
    const pool = n % 3 === 0 ? mixed : letters;
    const len = 6 + Math.floor(rnd() * 12);
    let name = '';
    for (let i = 0; i < len; i++) name += pool[Math.floor(rnd() * pool.length)];
    if (n % 5 === 4) name = name.slice(0, 4) + '-' + name.slice(4, 8) + '-' + name.slice(8) + '-x';
    const sub = n % 7 === 6 ? 'cdn' + Math.floor(rnd() * 90) + '.' : '';
    out.push((n % 2 ? 'http://' : 'https://') + sub + name + '.' + tlds[Math.floor(rnd() * tlds.length)] + '/');
  }
  return out;
}

// Everything else the address parser can be handed.
const MISC = [
  'http://paypal.com@evil.com/', 'https://www.google.com@192.0.2.5/', 'https://user:pass@example.com/', 'https://user@example.com/',
  'https://paypal.com./', 'HTTPS://WWW.PAYPAL.COM/', 'https://example.com:8443/', 'http://localhost:3000/', 'http://dev.localhost/',
  'http://my.app.localhost:8080/', 'ftp://example.com/', 'javascript:alert(1)', 'data:text/html,hi', 'file:///C:/x.html',
  'mailto:a@b.com', 'chrome://settings', 'about:blank', 'not a url', 'http://', 'https://my-very-long-hyphen-name-here.com/',
  'https://123456.com/', 'https://a1b2c3.com/', 'https://0-0-0.net/', 'https://x1.com/', 'https://abc.com/', 'https://a-b.c-d.com/',
  'https://www.paypal.com/cgi-bin/webscr?cmd=_login', 'https://paypal.com/?redirect=https://evil.example/', 'https://exa_mple.com/',
  'https://-foo-.com/', 'https://a.b-.c/', 'https://foo.constructor-x.com/', 'https://login.foo.bar.baz/', 'https://ws/',
  'https://secure-verify-account-update.com/', 'https://support.apple.com.example.com/', 'https://www.microsoftonline-auth.com/',
  'https://alert.wallet-recovery.info/', 'http://paypal.com%2eevil.com/', 'https://www.xn--googl-fsa.com/', 'https://googlé.com/',
  'https://xn--90a3ac.xn--p1ai/', 'https://www.sparkasse.de/', 'https://www.ing.nl/', 'https://www.revolut.com/',
  'https://www.wise.com/', 'https://www.n26.com/', 'https://www.deutsche-bank.de/', 'https://www.barclays.co.uk/',
  'https://www.hsbc.com/', 'https://www.santander.co.uk/', 'https://www.lloydsbank.com/', 'https://www.natwest.com/',
  'https://online.tbcbank.ge/', 'https://ibank.bog.ge/', 'https://my.gov.ge/', 'https://www.dpd.com/', 'https://www.gls-group.eu/',
  'https://www.vodafone.com/', 'https://www.t-mobile.com/', 'https://www.att.com/', 'https://www.verizon.com/', 'https://www.ryanair.com/',
  'https://www.lufthansa.com/', 'https://www.emirates.com/', 'https://www.delta.com/', 'https://www.united.com/', 'https://www.aa.com/'
];

export const URLS = [
  ...LEGIT,
  ...BRAND_DOMAINS.flatMap(lookalikes),
  ...HOMOGLYPH,
  ...BRAND_KEYWORD,
  ...IP,
  ...RISKY_TLD,
  ...DEEP,
  ...randomHosts(60),
  ...MISC
];

// A synthetic community feed (never real threat data): a small Bloom filter and
// a blocklist, so the feed paths are compared too.
export const FEED_BLOOM_DOMAINS = [
  'evil-bank-login.com', 'totally-legit-prizes.net', 'phish.example-host.org', 'random-name.tk', 'shop.xyz', 'x1.com',
  'secure-verify-account-update.com', 'deals.top'
];
export const FEED_BLOCKLIST = ['feed-blocked.example', 'another-bad.test', 'paypa1-secure.net'];
export const FEED_URLS = [
  'https://evil-bank-login.com/', 'https://www.evil-bank-login.com/login', 'https://totally-legit-prizes.net/', 'https://phish.example-host.org/',
  'https://other.example-host.org/', 'https://random-name.tk/', 'https://shop.xyz/', 'https://x1.com/', 'https://secure-verify-account-update.com/',
  'https://deals.top/', 'https://feed-blocked.example/', 'https://a.b.feed-blocked.example/', 'https://another-bad.test/', 'https://paypa1-secure.net/',
  'https://paypal-verify.com/', 'https://www.google.com/', 'https://nothing-here.example/', 'https://my.feed-blocked.example.com/'
];

// Page facts the probe reports, on top of an address. Shapes match probe.js.
const MS_HINTS = { title: 'Sign in to your Microsoft account', ogSiteName: '', logoAlts: [], brandTokens: [] };
export const PAGES = [
  { url: 'https://login-portal-example.com/', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Welcome', ogSiteName: '', logoAlts: [], brandTokens: [] }, resourceHosts: {}, faviconCrossOrigin: false, formActions: [] } } },
  { url: 'https://www.nytimes.com/login', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Log in - The New York Times' }, resourceHosts: {}, faviconCrossOrigin: false, formActions: [] } } },
  { url: 'https://evil-proxy.com/login', facts: { aitm: { interactions: ['password'], identityHints: MS_HINTS, resourceHosts: { 'evil-proxy.com': 10, 'cdn.evil-proxy.com': 3 }, faviconCrossOrigin: true, formActions: ['api.telegram.org'] } } },
  { url: 'https://evil-proxy.com/login', facts: { aitm: { interactions: ['otp'], identityHints: MS_HINTS, resourceHosts: { 'evil-proxy.com': 2, 'login.microsoftonline.com': 5 }, faviconCrossOrigin: false, formActions: [] } } },
  { url: 'https://evil-proxy.com/login', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Portal', ogSiteName: 'PayPal', logoAlts: ['PayPal logo'], brandTokens: ['sign in with google'] }, resourceHosts: { 'evil-proxy.com': 20 }, faviconCrossOrigin: false, formActions: ['collector.other-site.net'] } } },
  { url: 'https://example.okta.com/signin', facts: { aitm: { interactions: ['password'], identityHints: MS_HINTS, resourceHosts: {}, faviconCrossOrigin: true, formActions: ['evil.example'] } } },
  { url: 'https://account.microsoft.com/', facts: { aitm: { interactions: ['password'], identityHints: MS_HINTS, resourceHosts: {}, faviconCrossOrigin: false, formActions: [] } } },
  { url: 'https://secure-login-page.net/', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Sign in' }, resourceHosts: {}, faviconCrossOrigin: false, formActions: ['192.0.2.9'] } } },
  { url: 'https://secure-login-page.net/', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Sign in' }, resourceHosts: {}, faviconCrossOrigin: false, formActions: ['a.requestbin.net', 'b.example'] } } },
  { url: 'https://secure-login-page.net/', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Sign in' }, resourceHosts: {}, faviconCrossOrigin: false, formActions: ['auth.secure-login-page.net', 'accounts.google.com', 'okta.com', 'zscaler.net'] } } },
  { url: 'https://secure-login-page.net/', facts: { aitm: { interactions: ['password'], identityHints: { title: 'Sign in' }, resourceHosts: {}, faviconCrossOrigin: false, formActions: ['FORMS.Example.org', 'second.example'] } } },
  { url: 'https://outlook.com/', facts: { aitm: { interactions: ['password'], identityHints: MS_HINTS, resourceHosts: {}, faviconCrossOrigin: false, formActions: ['login.live.com'] } } },
  { url: 'https://brand-sso-test.net/', facts: { aitm: { interactions: ['passkey'], identityHints: { title: 'Apple ID' }, resourceHosts: { '10.0.0.1': 3, '[::1]': 1, 'brand-sso-test.net': 1 }, faviconCrossOrigin: true, formActions: ['webhook.site'] } } },
  { url: 'https://brand-sso-test.net/', facts: { aitm: { interactions: [], identityHints: MS_HINTS, resourceHosts: {}, faviconCrossOrigin: true, formActions: ['webhook.site'] } } },
  { url: 'https://wallet-restore.app/', facts: { scam: { cryptoSeed: true, seedInput: false, techScare: false, phone: false, fullscreen: false } } },
  { url: 'https://wallet-restore.app/', facts: { scam: { cryptoSeed: false, seedInput: true, techScare: true, phone: false, fullscreen: true } } },
  { url: 'http://pc-locked-alert.xyz/', facts: { scam: { cryptoSeed: false, seedInput: false, techScare: true, phone: true, fullscreen: false } } },
  { url: 'http://pc-locked-alert.xyz/', facts: { scam: { cryptoSeed: false, seedInput: false, techScare: true, phone: false, fullscreen: false } } },
  { url: 'https://www.google.com/', facts: { scam: { cryptoSeed: true, seedInput: false, techScare: false, phone: false, fullscreen: false }, aitm: { interactions: ['password'], identityHints: MS_HINTS, resourceHosts: {}, faviconCrossOrigin: false, formActions: ['evil.example'] } } },
  { url: 'https://fresh-domain-example.com/', facts: { youngDomainDays: 3 } },
  { url: 'https://fresh-domain-example.com/', facts: { youngDomainDays: 0, gsbThreat: 'reasonGsbDeceptive' } },
  { url: 'https://malware-host.example/', facts: { gsbThreat: 'reasonGsbMalware' } },
  { url: 'https://paypal-login-verify.com/', facts: { youngDomainDays: 2, gsbThreat: 'reasonGsbDeceptive', aitm: { interactions: ['password', 'otp'], identityHints: { title: 'PayPal: Log in' }, resourceHosts: { 'paypal-login-verify.com': 9 }, faviconCrossOrigin: true, formActions: ['api.telegram.org'] }, scam: { cryptoSeed: true, seedInput: true, techScare: true, phone: true, fullscreen: true } } },
  { url: 'https://unknown-helpdesk.net/', facts: { deviceCode: true } },
  { url: 'https://www.microsoft.com/', facts: { deviceCode: true } },
  { url: 'https://www.nytimes.com/', facts: { deviceCode: true } },
  { url: 'http://192.0.2.1/', facts: { deviceCode: true, youngDomainDays: 1 } },
  
  { url: 'https://paypal.com./', facts: { aitm: { interactions: ['password'], identityHints: { title: 'PayPal' }, resourceHosts: {}, faviconCrossOrigin: false, formActions: [] } } },
  { url: 'https://github-login.example/', facts: { aitm: { interactions: ['password'], identityHints: { title: '', ogSiteName: '', logoAlts: ['GitHub'], brandTokens: [] }, resourceHosts: { 'github-login.example': 4, 'x.example': 1 }, faviconCrossOrigin: false, formActions: [] } } }
];

// Page text for the probe's matchers (they run in the page in the extension).
export const TEXTS = [
  { text: 'Enter your password', title: '' },
  { text: 'Enter the one-time password we sent', title: '' },
  { text: 'Enter the verification code', title: '' },
  { text: 'Approve this sign-in request on your phone', title: '' },
  { text: 'Open your authenticator app', title: '' },
  { text: 'Migrate your MFA to the new app', title: '' },
  { text: 'Set up a passkey', title: '' },
  { text: 'Sign in with Windows Hello', title: '' },
  { text: 'Configure single sign-on', title: '' },
  { text: 'Scan the QR code with your phone to sign in', title: '' },
  { text: 'Security upgrade required', title: '' },
  { text: 'Verify your account to continue', title: '' },
  { text: 'პაროლი', title: '' },
  { text: 'Введите пароль', title: '' },
  { text: 'Passphrase', title: '' },
  { text: 'Forgot username?', title: '' },
  { text: 'Please enter your 12-word secret recovery phrase to restore your wallet.', title: '' },
  { text: 'We will never ask you to enter your seed phrase.', title: '' },
  { text: 'Import your wallet: paste the mnemonic phrase below', title: '' },
  { text: 'Never share your recovery phrase with anyone.', title: '' },
  { text: 'Confirm your private key. Do not share your private key.', title: '' },
  { text: "don't type your seed words anywhere. Type your seed words here", title: '' },
  { text: 'Your computer has been infected with a virus! Call Microsoft support now at 1-888-555-0199', title: '' },
  { text: 'WINDOWS DEFENDER: security alert. Do not restart your PC. Call toll-free: 1 (800) 555 0101', title: '' },
  { text: 'Trojan spyware detected. Contact the technician.', title: '' },
  { text: 'Security alert: suspicious sign-in attempt on your account', title: '' },
  { text: 'Your device is locked. dial support number', title: '' },
  { text: 'ვირუსი აღმოჩენილია დარეკეთ', title: '' },
  { text: 'Ваш компьютер заблокирован позвоните', title: '' },
  { text: 'Go to microsoft.com/link and enter the code ABCD-EFGH to continue.', title: 'Verify your identity' },
  { text: 'Go to microsoft.com/link and enter the code ABCD-EFGH to continue.', title: 'How to spot device code phishing' },
  { text: 'Visit google.com/device and type code X7K2M9QPZ', title: '' },
  { text: 'Visit google.com/device and type the code shown on your TV', title: '' },
  { text: 'Open https://microsoft.com/devicelogin. Use this code: HJ29-KD83P', title: 'Teams meeting' },
  { text: 'amazon.com/code — enter the code 12345678 now', title: '' },
  { text: 'Just a normal page about cats.', title: 'Cats' },
  { text: '', title: '' }
];

// Links a page shows, for the link checks (links.js).
export const LINKS = [
  { deep: false, links: [{ href: 'https://evil.example/login', text: 'www.paypal.com', download: '' }] },
  { deep: false, links: [{ href: 'https://www.paypal.com/', text: 'paypal.com', download: '' }] },
  { deep: false, links: [{ href: 'https://bit.ly/3xyz', text: 'click here', download: '' }] },
  { deep: false, links: [{ href: 'https://files.example/report.exe', text: 'report', download: 'report.pdf' }] },
  { deep: false, links: [{ href: 'https://files.example/report.pdf', text: 'report', download: 'report.pdf' }] },
  { deep: false, links: [{ href: 'https://www.google.com/', text: 'Visit https://microsoft.com now', download: '' }] },
  { deep: true, links: [{ href: 'https://paypal-login-verify.com/', text: 'Pay now', download: '' }, { href: 'https://www.wikipedia.org/', text: 'Wiki', download: '' }] },
  { deep: true, links: [{ href: 'https://login.microsoft.com.evil.xyz/', text: 'Sign in', download: '' }, { href: 'mailto:x@y.z', text: 'mail', download: '' }] },
  { deep: true, links: [{ href: 'https://evil.example/a', text: 'evil.example', download: '' }, { href: 'https://evil.example/a', text: 'evil.example', download: '' }] },
  { deep: false, links: [{ href: 'https://t.co/abc', text: 'news', download: '' }, { href: 'https://t.co/abc', text: 'news', download: '' }] },
  { deep: false, links: [{ href: 'https://cdn.example/setup.msi?x=1#y', text: 'Download', download: 'invoice.docx' }] },
  { deep: false, links: [{ href: 'https://cdn.example/archive/', text: 'Files', download: 'a.zip' }] },
  { deep: true, links: [{ href: 'https://www.bbc.co.uk/news', text: 'BBC.CO.UK', download: '' }] },
  { deep: true, links: [{ href: 'http://192.0.2.4/x', text: '192.0.2.4', download: '' }] }
];

export const DOWNLOADS = [
  { name: 'invoice.pdf.exe', mime: '' }, { name: 'photo.jpg.scr', mime: '' }, { name: 'setup.exe', mime: 'application/x-msdownload' },
  { name: 'report.pdf', mime: 'application/x-msdownload' }, { name: 'song.mp3', mime: 'application/x-msi' },
  { name: 'report.pdf', mime: 'application/pdf' }, { name: 'archive.tar.gz', mime: '' }, { name: 'x.docx', mime: 'application/java-archive' },
  { name: 'Readme', mime: '' }, { name: 'app.apk', mime: 'application/vnd.android.package-archive' }, { name: '.bashrc', mime: '' },
  { name: 'doc.txt', mime: 'application/x-sh' }, { name: 'doc.txt', mime: 'application/x-apple-diskimage' }, { name: 'SCRIPT.PS1', mime: '' }
];
