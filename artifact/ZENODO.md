# The Zenodo record of the artifact

**Pietro Sala** — Version 0.1

The checklist of the upload, for whoever mints the record. Everything
listed is staged in `~/Documents/sdcpi-tacas-results/` on the machine of
the campaign, beside the archives.

## Files of the record

| file | size | what it is |
|---|---:|---|
| `sdcpi-tacas-results-tool.zip` | 7.2 GB | the instances and the 181 440 runs of the tool, README and manifest inside |
| `sdcpi-tacas-results-prism-storm.zip` | 101 MB | the PRISM models, the queries and the runs of Storm 1.13.0, README and manifest inside |
| `SHA256SUMS` | 197 B | the checksums of the two archives |
| `sdcpi-tacas27-image-amd64.tar.gz` | 1.6 GB | the artifact image for linux/amd64, `docker load` restores it as `sdcpi-tacas27` |
| `sdcpi-parallel-<commit>.zip` | 117 MB | the snapshot of the repository the image is built from, `git archive` of the commit named inside |
| `README.md` | | a copy of `artifact/README.md`, so the record reads on its own |
| `LICENSE` | | MIT |

## Metadata

* Type: software. License: MIT. Language: English.
* Title: `Artifact of "A Parallel Algorithm for Strategy Synthesis in CPI" (TACAS 2027)`.
* Authors: Emanuele Chini (University of Verona and Sapienza University of
  Rome), Daniel Amadori (University of Verona), Pietro Sala (University of
  Verona), with their ORCID identifiers.
* Description: the abstract of `artifact/README.md`, first paragraph, with
  the badge line and the pointer to the repository.
* Related identifiers: `https://github.com/PietroSala/sdcpi-parallel` (is
  supplement to), and the DOI of the paper when there is one.

## After the record is minted

1. Write the DOI in `artifact/README.md`, in place of `[DOI to be
   minted]`, and push.
2. Write the DOI in the Data Availability Statement of the final version
   of the paper, as the comment there already provides.
3. Name the DOI in the artifact submission form, when the chairs announce
   the system of this year.
