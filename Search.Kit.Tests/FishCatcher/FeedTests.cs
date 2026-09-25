using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using SearchKit.FishCatcher;

namespace SearchKit.Tests;

/// The signed daily feed: signature, payload bytes, and the client's
/// keep-the-last-good-one behaviour. No network: a stand-in HttpClient
/// handler answers, and the fixture is signed with a throwaway test key.
public class FeedTests : IDisposable
{
    private static readonly JsonDocument Fixture = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "FishCatcher", "feed-fixture.json")));
    private static string TestKey => Fixture.RootElement.GetProperty("testKey").GetProperty("spki").GetString()!;
    private static JsonElement Bundle => Fixture.RootElement.GetProperty("bundle");
    private static byte[] BundleBytes => Encoding.UTF8.GetBytes(Bundle.GetRawText());

    private readonly string folder = Path.Combine(Path.GetTempPath(), "searchkit-feed-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        if (Directory.Exists(folder)) Directory.Delete(folder, recursive: true);
    }

    [Fact]
    public void The_payload_is_what_JSON_stringify_wrote()
    {
        // Key order (array indices first), 1e+21, 1e-7, -0 as 0, 5e-324, a raw
        // U+2028, "\u0007" and a lone surrogate: all byte for byte.
        Assert.Equal(Fixture.RootElement.GetProperty("payload").GetString(), FeedBundle.Payload(Bundle));
    }

    [Fact]
    public void A_bundle_signed_by_the_key_verifies()
    {
        var bundle = FeedBundle.Verify(BundleBytes, TestKey);
        Assert.NotNull(bundle);
        Assert.Equal(8, bundle.Count);
        Assert.NotNull(bundle.Bloom);
        Assert.True(bundle.Bloom.Has("evil-bank-login.com"));
        // Its blocklist isn't covered by the signature (see below).
        Assert.Null(bundle.BlockList);
    }

    [Fact]
    public void Another_key_or_a_changed_byte_is_refused()
    {
        // The registry's real key didn't sign the test fixture.
        Assert.Null(FeedBundle.Verify(BundleBytes, RegistryKey()));
        // Tampering with any signed field breaks it.
        var tampered = Bundle.GetRawText().Replace("\"count\": 8", "\"count\": 9").Replace("\"count\":8", "\"count\":9");
        Assert.NotEqual(Bundle.GetRawText(), tampered);
        Assert.Null(FeedBundle.Verify(Encoding.UTF8.GetBytes(tampered), TestKey));
        // No signature, a broken one, or not JSON at all.
        Assert.Null(FeedBundle.Verify(Encoding.UTF8.GetBytes("{\"version\":1,\"bloom\":{}}"), TestKey));
        Assert.Null(FeedBundle.Verify(Encoding.UTF8.GetBytes(Bundle.GetRawText().Replace("\"sig\": \"", "\"sig\": \"AAAA")), TestKey));
        Assert.Null(FeedBundle.Verify("not json"u8, TestKey));
        Assert.Null(FeedBundle.Verify(BundleBytes, "not a key"));
    }

    [Fact]
    public void A_blocklist_outside_the_signature_is_ignored()
    {
        // The fixture is signed the way remote.js signs (version, generated,
        // sources, count, bloom): its blocklist rides along unsigned, so anyone
        // who can change the bytes could add a warning for any site, or empty
        // the known-bad list. Only the signed Bloom filter is taken.
        var data = ParityTests.Data.Value;
        var fed = data.WithFeed(FeedBundle.Verify(BundleBytes, TestKey)!);
        Assert.Same(data.SafeList, fed.SafeList);
        Assert.Same(data.Brands, fed.Brands);
        Assert.Same(data.Keywords, fed.Keywords);
        Assert.DoesNotContain("feed-blocked.example", fed.BlockList);
        Assert.Equal(data.BlockList.Order(), fed.BlockList.Order());
        Assert.NotNull(fed.FeedBloom);
        Assert.Null(fed.WithoutFeed().FeedBloom);

        var v = Analyzer.Analyze("https://feed-blocked.example/", fed)!;
        Assert.DoesNotContain(v.Signals, s => s.Key == "reasonBlocklist");
    }

    [Fact]
    public async Task An_older_signed_feed_does_not_replace_a_newer_one()
    {
        // A replayed feed is still signed; only its date gives it away.
        using var key = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var spki = Convert.ToBase64String(key.ExportSubjectPublicKeyInfo());
        var now = new DateTimeOffset(2026, 9, 25, 8, 0, 0, TimeSpan.Zero);
        var server = new FakeServer(_ => Ok(Signed(key, "2026-09-25T06:00:00Z", 8), null));
        var client = new FeedClient(new HttpClient(server), folder, spki, "https://feed.test/list.json", () => now);
        Assert.Equal(FeedOutcome.Updated, (await client.RefreshAsync()).Outcome);

        server.Answer = _ => Ok(Signed(key, "2026-09-18T06:00:00Z", 5), null);
        Assert.Equal(FeedOutcome.Failed, (await client.RefreshAsync(force: true)).Outcome);
        Assert.Equal("2026-09-25T06:00:00Z", client.LoadSaved()!.Generated);
        Assert.Equal(8, client.State().Count);

        // The same feed again, or a newer one, is fine.
        server.Answer = _ => Ok(Signed(key, "2026-09-25T06:00:00Z", 8), null);
        Assert.Equal(FeedOutcome.Updated, (await client.RefreshAsync(force: true)).Outcome);
        server.Answer = _ => Ok(Signed(key, "2026-09-26T06:00:00Z", 9), null);
        Assert.Equal(FeedOutcome.Updated, (await client.RefreshAsync(force: true)).Outcome);
        Assert.Equal("2026-09-26T06:00:00Z", client.LoadSaved()!.Generated);
    }

    [Fact]
    public void A_blocklist_inside_the_signature_adds_to_the_bundled_one()
    {
        using var key = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var spki = Convert.ToBase64String(key.ExportSubjectPublicKeyInfo());
        var data = ParityTests.Data.Value;
        var bundledOne = data.BlockList.First();

        var bundle = FeedBundle.Verify(Signed(key, "2026-09-25T06:00:00Z", 1, ["feed-blocked.example"], signBlockList: true), spki);
        Assert.NotNull(bundle);
        Assert.Equal(["feed-blocked.example"], bundle.BlockList);
        var fed = data.WithFeed(bundle);
        Assert.Contains("feed-blocked.example", fed.BlockList);
        Assert.Contains(bundledOne, fed.BlockList);
        Assert.Contains(Analyzer.Analyze("https://feed-blocked.example/", fed)!.Signals, s => s.Key == "reasonBlocklist");
        Assert.DoesNotContain("feed-blocked.example", fed.WithoutFeed().BlockList);

        // A signed empty list can't empty the bundled one either.
        var empty = FeedBundle.Verify(Signed(key, "2026-09-25T06:00:00Z", 1, [], signBlockList: true), spki)!;
        Assert.Equal(data.BlockList.Order(), data.WithFeed(empty).BlockList.Order());

        // The same list added after signing (the remote.js way) is dropped, the Bloom filter kept.
        var unsigned = FeedBundle.Verify(Signed(key, "2026-09-25T06:00:00Z", 1, ["feed-blocked.example"], signBlockList: false), spki)!;
        Assert.Null(unsigned.BlockList);
        Assert.NotNull(unsigned.Bloom);

        // And a list swapped after signing breaks the signature that covered it.
        var swapped = Encoding.UTF8.GetString(Signed(key, "2026-09-25T06:00:00Z", 1, ["feed-blocked.example"], signBlockList: true))
            .Replace("feed-blocked.example", "your-bank.example");
        Assert.Null(FeedBundle.Verify(Encoding.UTF8.GetBytes(swapped), spki));
    }

    [Theory]
    [InlineData("2026-09-18T06:00:00Z", "2026-09-25T06:00:00Z", true)]
    [InlineData("2026-09-25T06:00:00Z", "2026-09-25T06:00:00Z", false)]
    [InlineData("2026-09-26T06:00:00Z", "2026-09-25T06:00:00Z", false)]
    [InlineData("2026-09-25T08:00:00+03:00", "2026-09-25T06:00:00Z", true)]
    [InlineData(null, "2026-09-25T06:00:00Z", false)]
    [InlineData("2026-09-18T06:00:00Z", null, false)]
    [InlineData("yesterday", "2026-09-25T06:00:00Z", false)]
    public void Older_compares_the_signed_dates(string? candidate, string? saved, bool older) =>
        Assert.Equal(older, FeedClient.Older(candidate, saved));

    /// A bundle signed as the registry signs it, by a key made for the test;
    /// with a blocklist, signed with it or (as remote.js) without.
    private static byte[] Signed(ECDsa key, string generated, int count, string[]? blocklist = null, bool signBlockList = false)
    {
        var bundle = new System.Text.Json.Nodes.JsonObject
        {
            ["version"] = 3,
            ["generated"] = generated,
            ["sources"] = new System.Text.Json.Nodes.JsonArray("test"),
            ["count"] = count,
            ["bloom"] = System.Text.Json.Nodes.JsonNode.Parse(Bundle.GetProperty("bloom").GetRawText()),
        };
        if (blocklist is not null) bundle["blocklist"] = new System.Text.Json.Nodes.JsonArray([.. blocklist.Select(d => (System.Text.Json.Nodes.JsonNode?)d)]);
        using (var doc = JsonDocument.Parse(bundle.ToJsonString()))
        {
            var payload = Encoding.UTF8.GetBytes(FeedBundle.Payload(doc.RootElement, signBlockList));
            bundle["sig"] = Convert.ToBase64String(key.SignData(payload, HashAlgorithmName.SHA256, DSASignatureFormat.IeeeP1363FixedFieldConcatenation));
        }
        return Encoding.UTF8.GetBytes(bundle.ToJsonString());
    }

    [Fact]
    public void The_registry_key_parses_as_P256()
    {
        using var key = ECDsa.Create();
        key.ImportSubjectPublicKeyInfo(Convert.FromBase64String(RegistryKey()), out int read);
        Assert.Equal(256, key.KeySize);
        Assert.Equal(91, read);
        Assert.Equal(ECCurve.NamedCurves.nistP256.Oid.Value, key.ExportParameters(false).Curve.Oid.Value);
    }

    [Fact]
    public async Task A_refresh_saves_a_verified_feed_and_skips_the_network_for_a_day()
    {
        var now = new DateTimeOffset(2026, 9, 25, 8, 0, 0, TimeSpan.Zero);
        var server = new FakeServer(_ => Ok(BundleBytes, "\"v1\""));
        var client = new FeedClient(new HttpClient(server), folder, TestKey, "https://feed.test/list.json", () => now);

        var first = await client.RefreshAsync();
        Assert.Equal(FeedOutcome.Updated, first.Outcome);
        Assert.Equal(8, first.Count);
        Assert.NotNull(client.LoadSaved());
        Assert.Equal<(DateTimeOffset?, long?, string?)>((now, 8L, "\"v1\""), client.State());

        now = now.AddHours(23);
        Assert.Equal(FeedOutcome.Fresh, (await client.RefreshAsync()).Outcome);
        Assert.Single(server.Requests);

        // A day later it asks again, with the ETag; the registry says nothing changed.
        now = now.AddHours(2);
        server.Answer = _ => new HttpResponseMessage(HttpStatusCode.NotModified);
        Assert.Equal(FeedOutcome.NotModified, (await client.RefreshAsync()).Outcome);
        Assert.Equal("\"v1\"", server.Requests[^1].Headers.IfNoneMatch.Single().ToString());
        Assert.NotNull(client.LoadSaved());
    }

    [Fact]
    public async Task Any_failure_keeps_the_last_good_feed()
    {
        var server = new FakeServer(_ => Ok(BundleBytes, null));
        var client = new FeedClient(new HttpClient(server), folder, TestKey, "https://feed.test/list.json");
        Assert.Equal(FeedOutcome.Updated, (await client.RefreshAsync()).Outcome);

        var failures = new Func<HttpRequestMessage, HttpResponseMessage>[]
        {
            _ => new HttpResponseMessage(HttpStatusCode.InternalServerError),
            _ => Ok("{\"not\":\"signed\"}"u8.ToArray(), null),
            _ => Ok(Encoding.UTF8.GetBytes(Bundle.GetRawText().Replace("2026-09-25", "2026-09-26")), null),
            _ => Ok("<html>captive portal</html>"u8.ToArray(), null),
            _ => throw new HttpRequestException("offline"),
            _ => Ok(new byte[FeedClient.MaxBytes + 1], null),
        };
        foreach (var failure in failures)
        {
            server.Answer = failure;
            Assert.Equal(FeedOutcome.Failed, (await client.RefreshAsync(force: true)).Outcome);
            var kept = client.LoadSaved();
            Assert.NotNull(kept);
            Assert.Equal(8, kept.Count);
        }
    }

    [Fact]
    public async Task Nothing_saved_means_no_feed()
    {
        var client = new FeedClient(new HttpClient(new FakeServer(_ => throw new HttpRequestException("offline"))), folder, TestKey);
        Assert.Null(client.LoadSaved());
        Assert.Equal(FeedOutcome.Failed, (await client.RefreshAsync()).Outcome);
        Assert.Null(client.LoadSaved());
        client.Forget();
    }

    private static string RegistryKey()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "Search.Kit", "FishCatcher", "Data", "registry-key.json");
        using var doc = JsonDocument.Parse(File.ReadAllBytes(path));
        var spki = doc.RootElement.GetProperty("spki").GetString()!;
        // The same key the engine loads from its embedded copy.
        Assert.Equal(ParityTests.Data.Value.RegistryKeySpki, spki);
        return spki;
    }

    private static HttpResponseMessage Ok(byte[] body, string? etag)
    {
        var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(body) };
        if (etag is not null) response.Headers.ETag = new System.Net.Http.Headers.EntityTagHeaderValue(etag);
        return response;
    }

    private sealed class FakeServer(Func<HttpRequestMessage, HttpResponseMessage> answer) : HttpMessageHandler
    {
        public Func<HttpRequestMessage, HttpResponseMessage> Answer { get; set; } = answer;
        public List<HttpRequestMessage> Requests { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Requests.Add(request);
            return Task.FromResult(Answer(request));
        }
    }
}
