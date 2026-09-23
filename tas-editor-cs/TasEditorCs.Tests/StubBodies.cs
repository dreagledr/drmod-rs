using System.IO;
using System.IO.Compression;

/// Reading a request the way the mod does.
///
/// The editor's client gzips every request body (`ModApi.Gzipped`), so a test
/// double that wants the script as text — to answer by `name=`, or to assert the
/// body it was handed — has to unpack it first. Keeping that in one place is what
/// lets both stubs (`StubMod`, the `MenuSettler` double) stay about behaviour
/// instead of about `Content-Encoding`.
internal static class StubBodies
{
    /// The plaintext of a gzipped body.
    internal static byte[] Inflate(byte[] wire)
    {
        using var input = new MemoryStream(wire);
        using var gzip = new GZipStream(input, CompressionMode.Decompress);
        using var plain = new MemoryStream();
        gzip.CopyTo(plain);
        return plain.ToArray();
    }
}
