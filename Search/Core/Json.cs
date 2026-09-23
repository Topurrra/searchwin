using System.Text.Json.Serialization;

namespace Search;

/// Every file this browser keeps, described to the compiler ahead of time.
///
/// The Mac's Codable is resolved when the app is compiled; .NET's JSON is
/// resolved by reflection when it runs, unless told otherwise. Told here: the
/// readers and writers are generated at build time, which is what lets the
/// app be compiled to native code (see publish-aot.cmd) and what saves the
/// first read of history and the session from warming reflection up first.
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    WriteIndented = false)]
[JsonSerializable(typeof(Session.Shape))]
[JsonSerializable(typeof(List<Visit>))]
[JsonSerializable(typeof(List<Bookmark>))]
[JsonSerializable(typeof(List<Keep>))]
[JsonSerializable(typeof(List<Space>))]
[JsonSerializable(typeof(Dictionary<string, List<Veil>>))]
[JsonSerializable(typeof(List<Installed>))]
[JsonSerializable(typeof(string))]
internal sealed partial class Json : JsonSerializerContext;
