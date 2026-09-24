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
        Assert.Equal(["feed-blocked.example", "another-bad.test", "paypa1-secure.net"], bundle.BlockList);
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
    public void Fields_outside_the_signature_cannot_change_the_verdict_data_silently()
    {
        // blocklist isn't signed (remote.js signs version, generated, sources,
        // count, bloom), as in the extension; what matters is that a verified
        // bundle can only replace the blocklist and the Bloom filter, never the
        // safe list, brands or keywords.
        var data = ParityTests.Data.Value;
        var fed = data.WithFeed(FeedBundle.Verify(BundleBytes, TestKey)!);
        Assert.Same(data.SafeList, fed.SafeList);
        Assert.Same(data.Brands, fed.Brands);
        Assert.Same(data.Keywords, fed.Keywords);
        Assert.Contains("feed-blocked.example", fed.BlockList);
        Assert.DoesNotContain("feed-blocked.example", fed.WithoutFeed().BlockList);
        Assert.Null(fed.WithoutFeed().FeedBloom);
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
