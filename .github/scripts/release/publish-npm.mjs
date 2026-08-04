import {
  listPackageTarballs,
  readPackageManifest,
  runNpm,
  spawnNpm,
} from "./utils.mjs";

const packages = listPackageTarballs("npm-packages").map((tarball) => ({
  tarball,
  manifest: readPackageManifest(tarball),
}));

if (packages.length < 10) {
  throw new Error("Expected eight native packages and two root packages");
}

function publishIfMissing({ tarball, manifest }) {
  const packageVersion = `${manifest.name}@${manifest.version}`;
  const result = spawnNpm(["view", packageVersion, "version"], {
    stdio: "ignore",
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status === 0) {
    console.log(`${packageVersion} is already published`);
    return;
  }
  if (result.signal) {
    throw new Error(
      `npm view ${packageVersion} terminated with ${result.signal}`,
    );
  }

  runNpm([
    "publish",
    tarball,
    "--access",
    "public",
    "--provenance",
    "--tag",
    "latest",
  ]);
}

const nativePackages = packages.filter(({ manifest }) =>
  manifest.name.startsWith("@microsoft/shell-use-"),
);
const rootPackages = packages.filter(
  ({ manifest }) =>
    manifest.name === "@microsoft/shell-use" || manifest.name === "shell-use",
);

for (const packageArtifact of [...nativePackages, ...rootPackages]) {
  publishIfMissing(packageArtifact);
}
