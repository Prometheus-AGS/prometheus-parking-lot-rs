# Windows Source Identity Implementation Plan

> For agentic workers: use the established KBD task2/native worker contract; root owns boundaries and final integration.

**Goal:** Make tracked symlink identity portable between native Windows and POSIX without accepting altered link targets.

**Architecture:** Verify actual link/materialized target against its exact canonical Git index blob, allowing only the observed Windows separator representation. Preserve strict full-source equality in the collector.

**Tech Stack:** Python stdlib, Git, existing hosted Windows/Linux workflow. No new dependency.

**Spec:** Adopted original software contract04 scenarioOA15 plus actual rehearsal40184361135 packet and Windows checked-manifest in private candidate evidence.

**Review focus:** native Windows link separators; POSIX backslash literals; regular materialized link bytes; wrong/missing target; canonical Git blob replacement masking.

- [ ] Implement discriminating link fixtures and minimum canonical-identity repair as one coherent set.
- [ ] Run completed packet/storage/manual fixture gate once; inspect results.
- [ ] Obtain independent source/security review.
- [ ] Prove real Windows producer to Linux collector on exact fixed commit.
- [ ] Secret scan/sign/review diff, deliver ordinary PR, freeze actual new merged candidate.
- [ ] Run all required final certification and actual publication/consumer checks.

Phase phase-0-4-operational-assurance, Execute, oa-20 task2 already active. User authorizes autonomous completion; budget-only signed bindingrevision3 is b1993dbc..., all17scenariooracles unchanged. Software source remains original04 contract until source fix is delivered.

## Observed delta
First genuine b43 rehearsal40184361135: Linux61/61, macOS5/5, iOS4/4, Android4/4, Windows producer2/2; baseline40PASS/OA04bootstrapBLOCKED. Collector preservesWindowsBLOCKED/OA15BLOCKED because sixmode120000license symlink targets hash differently onWindows. NativeWindows os.readlink returns backslash form, hash1afdb6321d585f313169e50052dcbc188da757341e2280214009ff1127f5d0b2. ExactGit blob '../../LICENSE-MIT' hash94949dd83279f3ca3d0ce42ce0fc154d0ed817444454ee2925e903eec93949b6. Allother2790sourcehashes agree. Originalarchives/receipts retained unchanged; no fakePASS. ActualOA04controls on genuine b43 compact12598470802 passed18/18 with noCargo/publication, useful historical evidence only.

## Coherent repair
Ownerplatformworker uses clean verificationworktree branchcodex/windows-source-link-identity basedexactb43. Own .github/scripts/operational-assurance.py and .github/scripts/tests/boundary-packet.py only. Root owns diagnosticworkflow, canonicalKBDrecords and delivery. Read active team/roles and minimal Python/review guidance; noRustchanges. Compare actual tracked link against exact indexGit blob; only nativeWindows separator materialization is portable, never ignorechanged/missing/differenttarget. Preservebyte-exact POSIX and regular-file materialization checks. Do not followreferents or normalizearbitraryfilecontents. Disable Git replacement-object masking on new canonicalblob reads at this real sourceidentity boundary.

Add discriminating fixtures for actualPOSIXsymlink, Windows native targetrepresentation, regularlinkmaterialization, wrongtarget/alteredblob/missinglink rejection. Finishcoherent source+fixtures before meaningfulverification. Root serializes gates: packet/storage/manual fixtures plus realWindowsproducer->Linuxcollector diagnostic on exactfixedcommit. Independentcode/securityreview aftercompletefix.

## Delivery and final gate
Retainb43failure. Newsourcecommit/normalPR/secret-scan/signature/remoteCI needed; newmergedcandidate invalidatesaffected strictSHAreceipts. Rebuildsource/G02-G04/tag/GUI/quiet as required by exactidentityprotocol, no relabeling. Fullfinalcertification requires all17oracles and sixsame-runautomaticprofiles plus nativeGUI/quiet/R2. Credentialrotation/historypurge remainDEFERRED. No paid/customrunner, globalconfigchanges, validatorwaiver or mainforcepush.

## Bootstrap assessment pending
Rootarchitect is checking whether a separate genuine NOT_EXERCISED-only protected-main rehearsal can break OA04prior-artifact dependency without duplicating allproduction tests. No bootstrap implementation authorized by this subplan until concrete design/negative controls are reviewed and appended. Finalcertification cannotloseanymandatorycommand or same-runprovenance.


## Selected bootstrap design — same coherent pipeline repair
Root selected the architect-assessed additive nonpublishing bootstrap. Existing shared consumer already permits all-oracles-NOT_EXERCISED rehearsal and categorically rejects it for publication. Existing assembler cannot break its OA04prior-artifact circularity without a completed fullfirst-run. SourcefixnewSHA wouldotherwise repeatfull~2hourLinuxrun solelytoprovidebootstrap. No finalproductiontest is waived.

Ownerplatformworker additionally owns bootstrap-rehearsal subcommand in operational-assurance.py plus NEW tests/rehearsal-bootstrap.py. Root owns operational-assurance.yml. Do not change compactprotocol, sharedconsumer, fullassembler, automatic/manualprovenance validators or mandatorycommandinventory.

ProducerCLI: bootstrap-rehearsal --candidate C --contract sourcecontract --policy stagedpolicy --inputs stagedinputs --tag approvedtag --preparation-plan exactstagedoriginalplan --out FRESH_EXTERNAL. Purpose is CONSTANT rehearsal, no oracle/PASS/arbitraryartifact options. It must requireactualfirst-attemptprotected-main operational-assurance workflow/GitHub-hosted context using existingvalidate_run+actualAPIworkflowdigest+actualapprovals/currentGITHUB_SHA/ref/repo/workflow_ref; rootworkflowjobcondition bindsprotectedref andBOOTSTRAP_RUNNER_ENVIRONMENT derivesrunner.environment. No fake API/local positive accepted as completion.

Requirecleancandidate, validoriginalsoftwarecontract04+modelbinding, independentlypinnedpolicy, approvedauthorityG02/G03/G04 PASS with actualretainedrawproof; realtagverification and exactG03tag/publictrustidentity. Originalplanmustmatch protectedcollector inputmaterialreference + independentlyconfigured materialsha + resolvedpreparedpath, thenactualvalidate_preparation; retainbyteexactoriginal. The user-approved budgetb199 remainscanonicalKBDauthority, notanewreleaseCLI budgetoverride. Preserve/authenticateitslineage inphaseclosure/finalR2 throughexistingKBDmechanism. Do notintroducenewbudgetinput/waiver orchangecontract() toacceptarbitraryrevisions.

Emitexactexistingcompactfields andall17scenario IDs, every scenario/baseline/review/operatorgate NOT_EXERCISED withnopassingexecutionreceipt. Overallpurpose rehearsal/statusNOT_EXERCISED. Actualtag/signature/preparation identitiesreal. manifest_sha256 hashesretainedinspectionmanifest explicitlynotingnoproductionprofilesexecuted, currentGit/source/definition/policy/material/authority/provenanceidentity, notsixfabricatedenvironmentmanifests. Validate_receipt(publication=False). Onlycompacttwo protocolmembersinincumbentboundaryartifact; actualinspection/authorityproofs separateartifact. Recheckcleansourcebeforecompletion. NoCargo/processkill/OIDC/publish/remoteartifactwritefromCLI.

Workflow: bootstrap_rehearsal boolean defaultfalse; dedicated bootstrapjob runsUbuntu24.04 existingrelease-certification environment/read-onlypermissions. Rejectbootstrap+purposecertification orbootstrap+run_quiet asfailedrun/noartifact. Excludeprofiles/quiet/assemblewhenbootstraptrue; defaultfullbehaviorunchanged. Stagethroughexistingprepare-hosted-assurance --profilecollector usingapprovedpublicvars. Newjobdoesnothaveid-tokenpermission. Outputboundtofixedoperational-assurance-boundary nameandseparaterehearsal-bootstrap-evidence. No user-content oracle/PASSflags.

Discriminatingcontrols: all-oraclesNE exactprotocol; publication/authorize rejection; missing/wrong APIcontext/attempt/workflowpin/approval/tag/plan/materialpin/authority rejection; outputfresh/external; noprofile/Cargo execution; invalidworkflow combinations fail; fullcertificationstillrejectsmissingautomatic/manualevidence. Finishallrelatedsource/workflow/fixtures, thenoneconsolidatedfixturebatch+independentcode/securityreview+actualWindowsproducer->Linuxcollector diagnostic. Normaldeliverysecret/signature/PR; newmergedcandidatefreshsource/nativeGUI/quiet/gates. Actualhostedbootstrap onnewmain then18realOA04controls; finalfullsame-runallprofiles+R2 stillrequired beforepublication.


## Selected single-full-run execution (conditional on actual late-pin proof)
After newmergedcandidateC exists, refresh exactsource policyworkflowpin, sourcecontrols/G02-G04/signeduniquetag/nativeGUI/quiet/manualbundles normally. Keepsoftwarecontract04identityandcanonicalbudgetb199lineage separate/authenticated. Create/stagefreshpublicauthority/materials/preparation bytes. Dispatchactualbootstrap_rehearsal=true purpose=rehearsal run_quiet=false onprotectedmainC; no productionprofiles or certificates.

Before approving pendingbootstrap environment, performminimalactuallate-variable-resolution check suggestedbyindependentarchitect: preservecurrentprotected OA18_MATERIALS_JSON/readback; add oneunused **collector-only material alias** pointingtoalreadyapprovedimmutableoriginalplanmemberand samebytes/digest. No newauthority/secret/settinggrant. Approvebootstrap; inspectretainedmaterial-staging-receipt.json foralias. Its presence proveschangedconfigafterdispatchpassedthe SAMEjob-level env vars placement usedbycollector. PreserveAPIconfiguration timestamps/old+newstatesandreceipt; removeunusedaliasbeforefullcertdispatch. Firstattemptonly, no rerun/relabel. Docs aloneambiguous: environmentvarsbecomeavailableafterrunnerdeclaration (contexts447), variables119sayswon'toverwriteexisting env/varscontexts. Realempiricalproofisrequiredbeforebettingfullrunonlatepins. URLs https://docs.github.com/en/actions/reference/workflows-and-actions/variables and https://docs.github.com/en/actions/reference/workflows-and-actions/contexts.

Download/verifyactualbootstrapimmutablecompact; runreal18OA04controls onthatNEWcandidateartifact. PrepareactualGUI/quiet/publishers/sourcegates butnofinalR2yet. DispatchONEpurpose=certification fullsixprofiles C. Afterallprofilesfinish, HOLDcollector environmentapproval (mustverifyactualpendingdeployment/jobstate; initialmatrixapprovalmustnotautostartcollector). Downloadallsixsame-runimmutableprofilearchives+actualAPIrun/jobs/approvals/artifactmetadata, hashverify/extract, importactualmanualGUI/quietthroughunchangedsourcevalidators. Builda LOCALpreviewfullpacket usingactualnewCsourceandreceiptsWITHOUT --trusted; keepBLOCKEDprovenance/no compact andexplainlocalpreviewdoesnotestablishremoteauthority. NeverfakeGITHUB_ACTIONS/sourcecontext.

Fresh-contextfinalR2receivesallactualnewCrawprofile/discovery/logs/manualbundles/OA04, localpreviewandactualauthenticatedGitHubmetadata. It mustindependentlyauthenticateall6same-runfirstattemptcandidate/sourceworkflow/envapproval/archiveIDs/digestsandverifyall8profiles throughproductionvalidators, thenreviewallactualoraclesandknownperformance/website/rotationlimits. It neednotreviewitsownfuturecertificate: finaltrustedcollector validationandSUCCESScompletionremainmandatoryafterreport. Create/verifycanonicalv0.4andfreshG03publicationreport AFTERfunctionalprofilespass, beforeR2acceptance (preserveoriginalaa32archive/exactoldreflease); don'tclaimR2reviewedfuturetagbytes. No source/definition/contractchangesafterprofiles.

OnceR2actualacceptanceexists, publiclytransport/pinrealR2+rawproofs+newcollector-onlyauthorityconfig (newfilename). KEEPallprofilematerials/mappingsandfrozeninputsunchanged; appendcollector-onlymaterials, changeonlycollectorrequiredreferences/tag, preservebothconfigs/chronologyandoldprofilepin. GlobalauthoritySHAchangesonlybeforecollectorstartwhileprofilesalreadycomplete. Readbackverifyactualprotectedvars. Approveexistingcollector onlythen; it usesUNCHANGEDtrustedfetch_jobs/fullassembleandmustallPASSpluswholeworkflowSUCCESS. Iflatepincheck orseparatependingcollector fails, no shortcut: usefreshdispatchwithallfinalinputsavailableorreviewasourceadjustmentBEFOREfreezingnewcandidate. No repeatedfirstattempt/relabel/syntheticPASS. No secondfullrunplannedsolelyforR2ifactualpause+latepinmechanismworks. Thenactualrelease.ymlrehearse->OIDCpublish->7registry/checksums/VCS/native+WASMconsumers->KBDend/closure.
