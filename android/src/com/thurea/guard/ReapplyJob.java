package com.thurea.guard;

import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;

/** Every 15 minutes: hide any newly installed browser, keep pins in force, put the settings back. */
public class ReapplyJob extends JobService {
    static void schedule(Context c) {
        JobScheduler js = c.getSystemService(JobScheduler.class);
        js.schedule(new JobInfo.Builder(1, new ComponentName(c, ReapplyJob.class))
                .setPeriodic(15 * 60 * 1000L).setPersisted(true).build());
    }
    /** In a few seconds (e.g. the YouTube ad blocker was just switched off). */
    static void soon(Context c) {
        c.getSystemService(JobScheduler.class).schedule(new JobInfo.Builder(2, new ComponentName(c, ReapplyJob.class))
                .setMinimumLatency(3000).setOverrideDeadline(10000).build());
    }
    @Override public boolean onStartJob(JobParameters p) {
        // Off the main thread: Policy.apply can wait on the network (family DNS), and Android
        // treats a slow onStartJob as the app hanging. Then: newer Morphe patches out? (only notifies)
        new Thread(() -> {
            try { MainActivity.saveStatus(this, Policy.apply(this)); Pins.checkForUpdate(this); Pins.updateMicrog(this, false); Updater.check(this); } finally { jobFinished(p, false); }
        }).start();
        return true;
    }
    @Override public boolean onStopJob(JobParameters p) { return true; }
}
