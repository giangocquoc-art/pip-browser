import { execFileSync } from "node:child_process";
import { shouldBuildWebsite } from "./vercel-build-policy.mjs";

const previousSha = process.env.VERCEL_GIT_PREVIOUS_SHA;
const currentSha = process.env.VERCEL_GIT_COMMIT_SHA;

// Unknown history must build so a deploy can never be skipped by guesswork.
if (!previousSha || !currentSha || /^0+$/.test(previousSha)) {
  console.log("Vercel build required: previous/current commit is unavailable.");
  process.exit(1);
}

try {
  const output = execFileSync(
    "git",
    ["diff", "--name-only", "--no-renames", previousSha, currentSha, "--"],
    { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  const changedPaths = output.split(/\r?\n/).filter(Boolean);

  if (shouldBuildWebsite(changedPaths)) {
    console.log("Vercel build required: website content or deployment config changed.");
    process.exit(1);
  }

  console.log("Skipping Vercel build: this commit does not change deployed website files.");
  process.exit(0);
} catch (error) {
  console.error("Vercel build required: unable to inspect changed paths.", error.message);
  process.exit(1);
}
