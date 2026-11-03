using Xunit;
namespace EmbedPDF.Tests;

public class WindowsLoaderTests
{
    [Fact]
    public void SourceTestsStageDependencyWhereProductionPreloadLooks()
    {
        var directory = Path.Combine(Path.GetTempPath(), "epdf-windows-staging-" + Guid.NewGuid());
        var runtime = Path.Combine(directory, "runtime");
        var output = Path.Combine(directory, "output");
        Directory.CreateDirectory(runtime);
        Directory.CreateDirectory(output);
        try
        {
            byte[] dependency = [1, 2, 3, 4];
            File.WriteAllBytes(Path.Combine(runtime, "embedpdf.dll"), dependency);
            FacadeTests.StageWindowsDependency(runtime, output);
            Assert.Equal(dependency, File.ReadAllBytes(Path.Combine(output, "embedpdf.dll")));
            File.Delete(Path.Combine(runtime, "embedpdf.dll"));
            Assert.Throws<FileNotFoundException>(() => FacadeTests.StageWindowsDependency(runtime, output));
        }
        finally { Directory.Delete(directory, true); }
    }
}
