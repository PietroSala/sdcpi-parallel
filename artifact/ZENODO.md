# The Zenodo record of the artifact

**Pietro Sala** — Version 0.4

The checklist of the upload, for whoever mints the record. Everything
listed is staged in `~/Documents/sdcpi-tacas-results/` on the machine of
the campaign, beside the archives.

The record 23587772 was published on 2026-10-23, with the DOI
[10.5281/zenodo.23587772](https://doi.org/10.5281/zenodo.23587772), the
metadata below and the three ORCID identifiers. Its eleven files are those
of the table, staged at commit 7781431, each checked by size and MD5
against the staging after the upload and again on the published record:
steps 1 to 4 of the order are done. The zip of step 5 was rebuilt at the
same commit, with the README that carries the DOI; its submission on
OpenReview and step 6 remain.

How the upload went, for a new version of the record: Zenodo answered 502
at random moments during the long uploads, from one second to forty
minutes into a request, and a cut upload stores nothing and leaves its
entry locked as "already being uploaded", so the entry is deleted and the
whole file sent again. Uploads in parts are opened but refused, and a
request without a User-Agent is refused as unusual traffic. The 7.7 GB
archive went through on the university network over IPv4 (`curl -4`), at
7 to 18 MB/s, where IPv6 gave 0.5 MB/s. Saving the form of the draft in
the browser drops a DOI reserved through the API since the page was
loaded: reload the page before saving.

## Files of the record

| file | size | what it is |
|---|---:|---|
| `README.md` | | a copy of `artifact/README.md`, so the record reads on its own |
| `LICENSE` | | MIT |
| `vm-setup.sh` | 2 kB | a copy of `artifact/vm-setup.sh`: Docker and the image in the TACAS 2027 virtual machine, offline |
| `sdcpi-tacas27-docker-amd64.tar` | 74 MB | the six packages of Ubuntu 26.04 that docker.io needs on the clean machine, amd64 |
| `sdcpi-tacas27-docker-arm64.tar` | 66 MB | the same for arm64 |
| `sdcpi-tacas27-image-amd64.tar.gz` | 1.6 GB | the artifact image for linux/amd64, `docker load` restores it as `sdcpi-tacas27` |
| `sdcpi-tacas27-image-arm64.tar.gz` | 1.7 GB | the same for linux/arm64 |
| `sdcpi-tacas-results-tool.zip` | 7.7 GB | the instances and the 181 440 runs of the tool, README and manifest inside |
| `sdcpi-tacas-results-prism-storm.zip` | 106 MB | the PRISM models, the queries and the runs of Storm 1.13.0, README and manifest inside |
| `SHA256SUMS` | 197 B | the checksums of the two archives, which `replicate.sh` checks |
| `sdcpi-parallel-<commit>.zip` | 117 MB | the snapshot of the repository the images are built from, `git archive` of the commit named inside |

The two images are built from the snapshot itself, so that the image and
the snapshot are one commit: in the folder it extracts to, for each
architecture in turn,
`docker build --platform linux/<arch> -f artifact/Dockerfile -t sdcpi-tacas27 .`
and then `docker save sdcpi-tacas27 | gzip > sdcpi-tacas27-image-<arch>.tar.gz`,
so that `docker load` restores the name every command of the README runs.
`SHA256SUMS-record`, beside the files, holds the checksums of all of them
but the two archives.

## Metadata

* Type: software. License: MIT. Language: English.
* Title: `Artifact of "A Parallel Algorithm for Strategy Synthesis in BPMN+CPI" (TACAS 2027)`.
* Authors: Emanuele Chini (University of Verona and Sapienza University of
  Rome), Daniel Amadori (University of Verona), Pietro Sala (University of
  Verona), with their ORCID identifiers.
* Description: the abstract of `artifact/README.md`, first paragraph, with
  the badge line and the pointer to the repository.
* Related identifiers: `https://github.com/PietroSala/sdcpi-parallel` (is
  supplement to), and the DOI of the paper when there is one.

## The order of the upload

1. Create the record as a draft and reserve its DOI, which Zenodo gives
   before publication.
2. Write the DOI in `artifact/README.md`, in place of `[DOI to be
   minted]`, commit and push.
3. Build the two images from the snapshot of that commit, save them, and
   write the snapshot; refresh `SHA256SUMS-record`.
4. Upload every file above to the draft, and publish.
5. Rebuild the zip of the submission form, `sdcpi-tacas27-artifact-submission.zip`,
   with the new README, and submit it on OpenReview with the number of the
   submission, 5092, and the paper as submitted.
6. Write the DOI in the Data Availability Statement of the final version of
   the paper, as the comment there already provides.
